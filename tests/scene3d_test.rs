use puzzle_ui::Puzzle3D;
use xrack_layout::{LayoutNode, Point};
use xrack_renderer_femtovg::{
    Renderer,
    preview::{PointerContact, PreviewScene},
};
use xrack_runtime::{
    Component, Node, Value, VecModel,
    scene3d::{Mesh3D, SceneCamera},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn viewport<'a>(node: &'a Node, layout: &'a LayoutNode) -> Option<(&'a Node, &'a LayoutNode)> {
    if node.kind == "SceneView3D" {
        return Some((node, layout));
    }
    layout
        .children_in_paint_order(node)
        .find_map(|(_, node, layout)| viewport(node, layout))
}

fn inspect(scene: &PreviewScene) -> TestResult<(&Node, &LayoutNode)> {
    let (node, layout, _) = scene
        .inspect_frame()
        .ok_or("preview needs a prepared frame")?;
    viewport(node, layout).ok_or_else(|| "missing SceneView3D".into())
}

#[test]
fn shared_scene_projection_preserves_row_reactivity_and_picking() -> TestResult {
    use scene3d_model_ui::{Actor3D, ProjectionReuseScene};
    use xrack_runtime::{Color, RecordList, RecordValue};

    const NAME: &str = "tests/scene3d_model.xr";
    let preview = xrack_compiler::prepare_preview(
        include_str!("scene3d_model.xr"),
        NAME,
        Default::default(),
    )?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ProjectionReuseScene")?;
    let mut generated = ProjectionReuseScene::new();

    let actor = |id: String, x, y, z| Actor3D {
        id,
        mesh: "primitive:box".into(),
        x,
        y,
        z,
        scale_x: 0.6,
        scale_y: 0.6,
        scale_z: 0.6,
        color: Color(0x40c2e8ff),
        ..Default::default()
    };
    let record = |row: &Actor3D| {
        RecordValue::new(
            "Actor3D",
            [
                ("id", Value::String(row.id.clone())),
                ("mesh", Value::String(row.mesh.clone())),
                ("x", Value::Float(row.x)),
                ("y", Value::Float(row.y)),
                ("z", Value::Float(row.z)),
                ("scale-x", Value::Float(row.scale_x)),
                ("scale-y", Value::Float(row.scale_y)),
                ("scale-z", Value::Float(row.scale_z)),
                ("yaw", Value::Float(row.yaw)),
                ("pitch", Value::Float(row.pitch)),
                ("roll", Value::Float(row.roll)),
                ("color", Value::Color(row.color)),
                ("group", Value::String(row.group.clone())),
            ]
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
        )
    };
    let mut actors = vec![
        actor("first".into(), 0.0, 0.0, 2.0),
        actor("second".into(), 0.0, 0.0, 0.0),
    ];
    for index in 0..512 {
        actors.push(actor(
            format!("box-{index}"),
            (index % 32) as f64 * 0.7 - 11.2,
            (index / 32) as f64 * 0.7 + 1.2,
            -2.0,
        ));
    }
    let record_rows = RecordList::new("Actor3D", actors.iter().map(record).collect());
    let aot_rows = VecModel::from(actors);
    generated.bind_model_actors(aot_rows.clone());
    interpreted.set("actors", Value::RecordList(record_rows.clone()))?;

    fn measured<T>(
        operation: impl FnOnce() -> TestResult<T>,
    ) -> TestResult<(T, xrack_runtime::update_profile::UpdateSample)> {
        let mut result = None;
        let profile = xrack_runtime::update_profile::measure(|| result = Some(operation()))
            .ok_or("missing update profile")?;
        Ok((result.ok_or("missing profiled result")??, profile))
    }
    fn shared_views(node: &Node) -> TestResult<std::sync::Arc<xrack_runtime::scene3d::SceneModel>> {
        let mut models =
            node.children
                .iter()
                .filter_map(|child| match child.properties.get("model") {
                    Some(Value::SceneObjects(model)) => Some(model),
                    _ => None,
                });
        let left = models.next().ok_or("missing left 3D model")?;
        let right = models.next().ok_or("missing right 3D model")?;
        assert!(std::sync::Arc::ptr_eq(left, right));
        Ok(left.clone())
    }

    let mut compare = |heading, picked: &str, source_row, builds| -> TestResult<_> {
        if generated.get_heading() != heading {
            generated.set_heading(heading);
            interpreted.set("heading", Value::Float(heading))?;
        }
        let ((aot, hit), aot_work) = measured(|| {
            let scene = shared_views(&generated.render())?;
            let hit = generated.get_hit();
            let rays = generated.get_ray_hits();
            assert_eq!(
                rays.first().map(|ray| ray.object_id.as_str()),
                Some(hit.object_id.as_str())
            );
            assert!(!generated.get_missing_camera().valid);
            Ok((scene, hit))
        })?;
        let ((dynamic, dynamic_hit), dynamic_work) = measured(|| {
            let tree = interpreted.try_render()?;
            let scene = shared_views(&tree)?;
            let hit = interpreted.get("hit")?;
            let Value::Hit3DList(rays) = interpreted.get("ray-hits")? else {
                return Err("incorrect interpreted ray result".into());
            };
            let Value::Hit3D(pointed) = &hit else {
                return Err("incorrect interpreted pick result".into());
            };
            assert_eq!(
                rays.row(0).map(|ray| ray.object_id.clone()),
                Some(pointed.object_id.clone())
            );
            let Value::ProjectedPoint3D(projection) = interpreted.get("missing-camera")? else {
                return Err("incorrect interpreted projection".into());
            };
            assert!(!projection.valid);
            Ok((scene, hit))
        })?;
        assert_eq!(hit.object_id, picked);
        assert_eq!(hit.row_index, source_row);
        assert_eq!(dynamic_hit, Value::Hit3D(hit));
        for work in [aot_work, dynamic_work] {
            assert_eq!(work.scene_projections_built, builds);
            assert!(work.scene_projection_cache_hits >= 3);
        }
        Ok([aot, dynamic])
    };

    let cold = compare(0.0, "first", 0, 1)?;
    let warm = compare(0.1, "first", 0, 0)?;
    for (before, after) in cold.iter().zip(&warm) {
        assert!(std::sync::Arc::ptr_eq(before, after));
    }
    let moved = actor("first".into(), 40.0, 0.0, 2.0);
    assert!(aot_rows.set_row(0, moved.clone()));
    assert!(record_rows.model().set_row(0, record(&moved)));
    compare(0.1, "second", 1, 1)?;
    assert!(aot_rows.move_row(1, 0));
    assert!(record_rows.model().move_row(1, 0));
    compare(0.1, "second", 0, 1)?;
    let removed = aot_rows.remove(0).ok_or("missing first row to remove")?;
    let removed_record = record_rows
        .model()
        .remove(0)
        .ok_or("missing interpreted row to remove")?;
    assert_eq!(removed.id, "second");
    assert_eq!(
        removed_record.field("id"),
        Some(&Value::String("second".into()))
    );
    assert!(aot_rows.insert(1, actor("replacement".into(), 0.0, 0.0, 0.0)));
    assert!(
        record_rows
            .model()
            .insert(1, record(&actor("replacement".into(), 0.0, 0.0, 0.0)))
    );
    compare(0.1, "replacement", 1, 1)?;
    eprintln!(
        "Both backends shared a single projection across two viewports and three queries (514 rows, zero warm rebuilds)"
    );
    Ok(())
}

#[test]
fn scene_lut_uses_all_three_axes_and_reuses_uploaded_pngs() -> TestResult {
    const NAME: &str = "examples/puzzle3d/lut.xr";
    const VIEW_BYTES: usize = 480 * 248 * 4;
    let invalid = r#"
        export component Invalid inherits Window {
            SceneView3D { scene-lut: @image-url("models/lut-identity.png"); scene-lut-size: 4; }
        }
    "#;
    let preview = xrack_compiler::prepare_preview(invalid, NAME, Default::default())?;
    let error = xrack_compiler::check_resources(&preview.module, NAME, None)
        .err()
        .ok_or("accepted a mismatched LUT size")?
        .to_string();
    assert!(
        error.contains("256×16") && error.contains("scene-lut-size 4"),
        "{error}"
    );

    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for file in ["lut-identity.png", "lut-invert.png", "lut-warm.png"] {
        assert!(inputs.iter().any(|path| path.ends_with(file)), "{file}");
    }
    let interpreted = program.create("LutDemo")?;
    let generated = lut_ui::LutDemo::new();
    let weak = generated.as_weak();
    let mut dynamic_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut aot_view = PreviewScene::new(Box::new(generated));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy)]
    struct View {
        preset: i64,
        strength: f64,
        size: i64,
        hdr: bool,
        saturation: f64,
    }
    let base = View {
        preset: 0,
        strength: 1.0,
        size: 0,
        hdr: false,
        saturation: 1.0,
    };
    let mut render = |view: View| -> TestResult<_> {
        let mut aot = weak.upgrade().ok_or("missing generated LUT demo")?;
        aot.set_preset(view.preset);
        aot.set_strength(view.strength);
        aot.set_requested_size(view.size);
        aot.set_hdr(view.hdr);
        aot.set_saturation(view.saturation);
        for (name, value) in [
            ("preset", Value::Int(view.preset)),
            ("strength", Value::Float(view.strength)),
            ("requested-size", Value::Int(view.size)),
            ("hdr", Value::Bool(view.hdr)),
            ("saturation", Value::Float(view.saturation)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot_view.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(
            png.as_slice() == dynamic_view.render(&mut renderer)?,
            "LUT output has AOT/Interpreter parity"
        );
        assert_eq!(
            after.hdr_resolves - before.hdr_resolves,
            u64::from(view.hdr)
        );
        let image = xrack_assets::decode_png(&png)?;
        assert_eq!(image.size, [480, 300]);
        Ok((
            image,
            [
                after.lut_resolves - before.lut_resolves,
                after.lut_uploads - before.lut_uploads,
                after.color_draws - before.color_draws,
                after.bloom_passes - before.bloom_passes,
            ],
        ))
    };
    let pixel = |image: &xrack_assets::Raster, x: usize, y: usize| -> [u8; 3] {
        std::array::from_fn(|channel| image.rgba[4 * (480 * y + x) + channel])
    };
    let assert_close = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 1),
            "{actual:?} vs {expected:?}"
        );
    };
    let (plain, work) = render(base)?;
    assert_eq!(&work[..2], &[0, 0]);
    let (identity, active) = render(View { preset: 1, ..base })?;
    assert_eq!(active, [1, 1, work[2], 0]);
    let (inverted, active) = render(View { preset: 3, ..base })?;
    assert_eq!(active, [1, 1, work[2], 0]);
    let (mixed, reused) = render(View {
        preset: 3,
        strength: 0.5,
        ..base
    })?;
    assert_eq!(reused, [1, 0, work[2], 0]);
    for (x, y) in [
        (8, 8),
        (62, 81),
        (240, 87),
        (240, 163),
        (310, 77),
        (370, 159),
    ] {
        let expected = pixel(&plain, x, y);
        assert_close(pixel(&identity, x, y), expected);
        assert_close(pixel(&inverted, x, y), expected.map(|v| 255 - v));
        assert_close(pixel(&mixed, x, y), [128; 3]);
    }
    let (warm, first) = render(View {
        preset: 2,
        size: 4,
        ..base
    })?;
    assert_eq!(first, [1, 1, work[2], 0]);
    let [r, g, b] = pixel(&plain, 8, 8).map(f32::from);
    assert_close(
        pixel(&warm, 8, 8),
        [23.0 + 0.9 * r, 10.0 + 0.86 * g, 7.0 + 0.69 * b].map(|channel| channel.round() as u8),
    );
    let (unmatched, rejected) = render(View {
        preset: 2,
        size: 16,
        ..base
    })?;
    assert_eq!(rejected, work);
    assert!(
        unmatched.rgba[..VIEW_BYTES] == plain.rgba[..VIEW_BYTES],
        "invalid dimensions preserve original pixels"
    );
    let (neutral, neutral_pass) = render(View {
        preset: 3,
        strength: 0.0,
        ..base
    })?;
    assert_eq!(neutral_pass, work);
    assert!(
        neutral.rgba[..VIEW_BYTES] == plain.rgba[..VIEW_BYTES],
        "zero LUT strength preserves original pixels"
    );

    let hdr = View {
        hdr: true,
        saturation: 0.0,
        ..base
    };
    let (gray_hdr, _) = render(hdr)?;
    let (graded_hdr, hdr_work) = render(View { preset: 3, ..hdr })?;
    assert_eq!(
        hdr_work,
        [1, 0, work[2], 9],
        "LUT follows bloom in the same resolve"
    );
    for (x, y) in [(8, 8), (240, 164), (240, 87)] {
        assert_close(
            pixel(&graded_hdr, x, y),
            pixel(&gray_hdr, x, y).map(|v| 255 - v),
        );
    }
    eprintln!("LUT validated three RGB axes, 16- and 4-point strips, and GPU texture reuse");
    Ok(())
}

#[test]
fn tone_curves_and_color_adjustments_match_aot_and_interpreter() -> TestResult {
    use xrack_runtime::SceneToneMap;
    const NAME: &str = "examples/puzzle3d/tone.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ToneDemo")?;
    let generated = tone_ui::ToneDemo::new();
    let weak = generated.as_weak();
    let mut dynamic_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut aot_view = PreviewScene::new(Box::new(generated));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy)]
    struct View {
        curve: SceneToneMap,
        exposure: f64,
        white: f64,
        brightness: f64,
        contrast: f64,
        saturation: f64,
        glow: bool,
        guide_blur: bool,
    }
    let base = View {
        curve: SceneToneMap::Reinhard,
        exposure: 0.7,
        white: 11.2,
        brightness: 1.0,
        contrast: 1.0,
        saturation: 1.0,
        glow: false,
        guide_blur: false,
    };
    let mut render = |view: View| -> TestResult<_> {
        let mut aot = weak.upgrade().ok_or("missing generated tone demo")?;
        aot.set_curve(view.curve);
        aot.set_exposure(view.exposure);
        aot.set_white_point(view.white);
        aot.set_brightness(view.brightness);
        aot.set_contrast(view.contrast);
        aot.set_saturation(view.saturation);
        aot.set_glow(view.glow);
        aot.set_guide_blur(view.guide_blur);
        for (name, value) in [
            ("curve", Value::SceneToneMap(view.curve)),
            ("exposure", Value::Float(view.exposure)),
            ("white-point", Value::Float(view.white)),
            ("brightness", Value::Float(view.brightness)),
            ("contrast", Value::Float(view.contrast)),
            ("saturation", Value::Float(view.saturation)),
            ("glow", Value::Bool(view.glow)),
            ("guide-blur", Value::Bool(view.guide_blur)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot_view.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(
            png.as_slice() == dynamic_view.render(&mut renderer)?,
            "typed curves and grades render identically"
        );
        assert_eq!(
            interpreted.get("curve")?,
            Value::SceneToneMap(aot.get_curve())
        );
        assert_eq!(
            after.hdr_resolves - before.hdr_resolves,
            1,
            "only one pane selects HDR"
        );
        let image = xrack_assets::decode_png(&png)?;
        assert_eq!(image.size, [480, 300]);
        Ok((
            image,
            [
                after.color_adjustment_resolves - before.color_adjustment_resolves,
                after.color_draws - before.color_draws,
                after.bloom_passes - before.bloom_passes,
                after.depth_of_field_passes - before.depth_of_field_passes,
                renderer.scene_mesh_uploads(),
            ],
        ))
    };

    let pixel = |image: &xrack_assets::Raster, x: usize, y: usize| -> [u8; 4] {
        std::array::from_fn(|channel| image.rgba[4 * (480 * y + x) + channel])
    };
    let (reinhard, counts) = render(base)?;
    let ldr = pixel(&reinhard, 475, 12);
    assert_eq!(ldr, [48, 67, 91, 255]);
    let mut signatures = std::collections::HashSet::new();
    for curve in [
        SceneToneMap::None,
        SceneToneMap::Linear,
        SceneToneMap::Reinhard,
        SceneToneMap::Aces,
        SceneToneMap::HejlDawson,
        SceneToneMap::Filmic,
    ] {
        let (frame, work) = render(View { curve, ..base })?;
        assert_eq!(
            work, counts,
            "tone curves reuse draw calls and uploaded meshes"
        );
        assert_eq!(pixel(&frame, 475, 12), ldr);
        let signature = [
            pixel(&frame, 6, 12),
            pixel(&frame, 119, 163),
            pixel(&frame, 119, 82),
        ];
        assert!(
            signatures.insert(signature),
            "{curve:?} has a distinct tone curve"
        );
    }
    let (unmapped, _) = render(View {
        curve: SceneToneMap::None,
        exposure: 0.0,
        ..base
    })?;
    assert_eq!(pixel(&unmapped, 6, 12), ldr);
    let (linear, _) = render(View {
        curve: SceneToneMap::Linear,
        exposure: 0.0,
        ..base
    })?;
    assert_eq!(pixel(&linear, 6, 12), [0, 0, 0, 255]);

    let (filmic, _) = render(View {
        curve: SceneToneMap::Filmic,
        ..base
    })?;
    let (normalized, _) = render(View {
        curve: SceneToneMap::Filmic,
        white: 4.0,
        ..base
    })?;
    assert!(pixel(&normalized, 6, 12)[0] > pixel(&filmic, 6, 12)[0]);

    let (dark, grade) = render(View {
        brightness: 0.0,
        ..base
    })?;
    assert_eq!(grade[0], 2);
    assert_eq!(pixel(&dark, 6, 12), [0, 0, 0, 255]);
    assert_eq!(pixel(&dark, 475, 12), [0, 0, 0, 255]);
    let (flat, _) = render(View {
        contrast: 0.0,
        ..base
    })?;
    let pivot = pixel(&flat, 6, 12);
    assert!(pivot[0].abs_diff(118) <= 1);
    assert_eq!(pixel(&flat, 119, 163), pivot);
    assert_eq!(pixel(&flat, 475, 12), pivot);

    let (gray, effects) = render(View {
        glow: true,
        guide_blur: true,
        saturation: 0.0,
        ..base
    })?;
    assert_eq!(&effects[..4], &[2, counts[1], 9, 3]);
    for x in [119, 360] {
        let [r, g, b, a] = pixel(&gray, x, 163);
        assert!(
            r == g && g == b && a == 255,
            "grade is applied after bloom and focus"
        );
    }
    eprintln!(
        "All six tone maps produced distinct swatch signatures; {} mesh draws reused",
        counts[1]
    );
    Ok(())
}

#[test]
fn depth_of_field_reuses_camera_depth_and_switches_the_sharp_plane() -> TestResult {
    const NAME: &str = "examples/puzzle3d/focus.xr";
    const VIEW_BYTES: usize = 480 * 248 * 4;
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(inputs.iter().any(|path| path.ends_with("focus-grid.png")));
    let interpreted = program.create("FocusDemo")?;
    let generated = focus_ui::FocusDemo::new();
    let weak = generated.as_weak();
    let mut dynamic_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut aot_view = PreviewScene::new(Box::new(generated));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy)]
    struct View {
        enabled: bool,
        orthographic: bool,
        glass: bool,
        hdr: bool,
        focus: f64,
        radius: f64,
        aa_scale: f64,
    }
    let base = View {
        enabled: false,
        orthographic: true,
        glass: false,
        hdr: false,
        focus: 12.0,
        radius: 24.0,
        aa_scale: 1.0,
    };
    let mut render = |view: View| -> TestResult<_> {
        let mut aot = weak.upgrade().ok_or("missing generated focus demo")?;
        aot.set_enabled(view.enabled);
        aot.set_orthographic(view.orthographic);
        aot.set_glass(view.glass);
        aot.set_hdr(view.hdr);
        aot.set_focus(view.focus);
        aot.set_radius(xrack_runtime::Length(view.radius));
        aot.set_aa_scale(view.aa_scale);
        for (name, value) in [
            ("enabled", Value::Bool(view.enabled)),
            ("orthographic", Value::Bool(view.orthographic)),
            ("glass", Value::Bool(view.glass)),
            ("hdr", Value::Bool(view.hdr)),
            ("focus", Value::Float(view.focus)),
            ("radius", Value::Length(xrack_runtime::Length(view.radius))),
            ("aa-scale", Value::Float(view.aa_scale)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot_view.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(
            png.as_slice() == dynamic_view.render(&mut renderer)?,
            "depth of field image parity"
        );
        let picked = if view.glass { "glass" } else { "middle" };
        assert_eq!(aot.get_picked(), picked);
        assert_eq!(interpreted.get("picked")?, Value::String(picked.into()));
        assert_eq!(
            after.hdr_resolves - before.hdr_resolves,
            u64::from(view.hdr)
        );
        assert_eq!(
            after.transparency_composites - before.transparency_composites,
            u64::from(view.glass)
        );
        let scale = view.aa_scale.clamp(1.0, 2.0);
        let supersampled = scale > 1.0;
        assert_eq!(
            after.multisample_resolves - before.multisample_resolves,
            u64::from(view.glass && !supersampled)
        );
        assert_eq!(
            after.supersample_resolves - before.supersample_resolves,
            u64::from(supersampled)
        );
        let source_pixels = if supersampled {
            (480.0 * scale) as u64 * (248.0 * scale) as u64
        } else {
            0
        };
        assert_eq!(
            after.supersample_pixels - before.supersample_pixels,
            source_pixels
        );
        assert_eq!(
            after.occlusion_depth_draws - before.occlusion_depth_draws,
            0
        );
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [480, 300]);
        Ok((
            raster,
            [
                after.depth_of_field_passes - before.depth_of_field_passes,
                after.depth_of_field_pixels - before.depth_of_field_pixels,
                after.color_draws - before.color_draws,
            ],
        ))
    };
    let pixel = |image: &xrack_assets::Raster, x: usize, y: usize| image.rgba[4 * (480 * y + x)];
    let contrast = |image: &xrack_assets::Raster, center: usize| -> u64 {
        (90..138)
            .flat_map(|y| (center - 28..center + 28).map(move |x| (x, y)))
            .map(|(x, y)| u64::from(pixel(image, x, y).abs_diff(pixel(image, x + 1, y))))
            .sum()
    };

    let (plain, ordinary) = render(base)?;
    assert_eq!(&ordinary[..2], &[0, 0]);
    // Only the checkerboard interior is sampled here. MSAA leaves the shader
    // samples there untouched, while a larger scene raster filters the pattern.
    for aa_scale in [1.5, 2.0] {
        let (smoothed, work) = render(View { aa_scale, ..base })?;
        assert_eq!(work, ordinary, "SSAA preserves mesh draw count");
        let changes = (90..138)
            .flat_map(|y| (210..270).map(move |x| (x, y)))
            .filter(|&(x, y)| pixel(&plain, x, y) != pixel(&smoothed, x, y))
            .count();
        assert!(
            changes > 200,
            "{aa_scale}× filtered {changes} interior pixels"
        );
        assert!(contrast(&smoothed, 240) < contrast(&plain, 240));
        eprintln!("{aa_scale}× SSAA filtered {changes} checkerboard interior pixels");
    }
    let (disabled, work) = render(View {
        aa_scale: 0.5,
        ..base
    })?;
    assert_eq!(work, ordinary);
    assert!(disabled.rgba == plain.rgba);

    let enabled = View {
        enabled: true,
        ..base
    };
    let (middle, focused) = render(enabled)?;
    assert_eq!(
        focused,
        [3, 59_520, ordinary[2]],
        "the effect filters half-resolution images without another mesh pass"
    );
    for (x, y) in (90..138).flat_map(|y| (210..270).map(move |x| (x, y))) {
        assert_eq!(pixel(&plain, x, y), pixel(&middle, x, y), "in-focus center");
    }
    let near_sharp = contrast(&plain, 122);
    let far_sharp = contrast(&plain, 358);
    let near_blur = contrast(&middle, 122);
    let far_blur = contrast(&middle, 358);
    assert!(
        near_blur * 2 < near_sharp && far_blur * 2 < far_sharp,
        "{near_blur}/{near_sharp}, {far_blur}/{far_sharp}"
    );
    assert!(
        pixel(&middle, 80, 110) > pixel(&plain, 80, 110),
        "defocused foreground spreads before the card edge"
    );

    let (near, _) = render(View {
        focus: 8.0,
        ..enabled
    })?;
    assert_eq!(contrast(&near, 122), near_sharp);
    assert!(contrast(&near, 240) * 2 < contrast(&plain, 240));
    let (far, _) = render(View {
        focus: 17.0,
        ..enabled
    })?;
    assert_eq!(contrast(&far, 358), far_sharp);
    let (zero, skipped) = render(View {
        radius: 0.0,
        ..enabled
    })?;
    assert_eq!(skipped, ordinary);
    assert!(zero.rgba[..VIEW_BYTES] == plain.rgba[..VIEW_BYTES]);

    let combined = View {
        orthographic: false,
        hdr: true,
        glass: true,
        ..base
    };
    let (clear, regular) = render(combined)?;
    let (processed, work) = render(View {
        enabled: true,
        ..combined
    })?;
    assert_eq!(work, [3, focused[1], regular[2]]);
    assert!(processed.rgba[..VIEW_BYTES] != clear.rgba[..VIEW_BYTES]);
    let (combined_ssaa, ssaa_work) = render(View {
        enabled: true,
        aa_scale: 1.5,
        ..combined
    })?;
    assert_eq!(ssaa_work, [3, 133_920, regular[2]]);
    assert!(combined_ssaa.rgba[..VIEW_BYTES] != processed.rgba[..VIEW_BYTES]);
    eprintln!(
        "Focus contrast near {near_sharp} → {near_blur}, far {far_sharp} → {far_blur}; {} half-resolution filter pixels",
        focused[1]
    );
    Ok(())
}

#[test]
fn hdr_bloom_uses_visible_radiance_and_matches_both_backends() -> TestResult {
    const NAME: &str = "examples/puzzle3d/bloom.xr";
    const VIEW_BYTES: usize = 480 * 248 * 4;
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("BloomDemo")?;
    let generated = bloom_ui::BloomDemo::new();
    let weak = generated.as_weak();
    let mut dynamic_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut aot_view = PreviewScene::new(Box::new(generated));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy)]
    struct View {
        glow: bool,
        hdr: bool,
        covered: bool,
        glass: bool,
        probe: bool,
        strength: f64,
        threshold: f64,
        levels: i64,
        exposure: f64,
        width: f64,
    }
    let base = View {
        glow: false,
        hdr: true,
        covered: false,
        glass: false,
        probe: false,
        strength: 0.75,
        threshold: 1.0,
        levels: 5,
        exposure: 1.0,
        width: 480.0,
    };
    let mut render = |view: View| -> TestResult<_> {
        let mut aot = weak.upgrade().ok_or("missing generated bloom demo")?;
        aot.set_glow(view.glow);
        aot.set_hdr(view.hdr);
        aot.set_covered(view.covered);
        aot.set_glass(view.glass);
        aot.set_probe(view.probe);
        aot.set_strength(view.strength);
        aot.set_threshold(view.threshold);
        aot.set_levels(view.levels);
        aot.set_exposure(view.exposure);
        aot.set_view_width(xrack_runtime::Length(view.width));
        for (name, value) in [
            ("glow", Value::Bool(view.glow)),
            ("hdr", Value::Bool(view.hdr)),
            ("covered", Value::Bool(view.covered)),
            ("glass", Value::Bool(view.glass)),
            ("probe", Value::Bool(view.probe)),
            ("strength", Value::Float(view.strength)),
            ("threshold", Value::Float(view.threshold)),
            ("levels", Value::Int(view.levels)),
            ("exposure", Value::Float(view.exposure)),
            (
                "view-width",
                Value::Length(xrack_runtime::Length(view.width)),
            ),
        ] {
            interpreted.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot_view.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(
            png.as_slice() == dynamic_view.render(&mut renderer)?,
            "the bloom backends diverged"
        );
        let pick = if view.glass {
            "glass"
        } else if view.covered {
            "cover"
        } else {
            "emitter"
        };
        assert_eq!(aot.get_picked(), pick);
        assert_eq!(interpreted.get("picked")?, Value::String(pick.into()));
        assert_eq!(
            after.multisample_resolves - before.multisample_resolves,
            u64::from(view.glass)
        );
        assert_eq!(
            after.transparency_composites - before.transparency_composites,
            u64::from(view.glass)
        );
        let image = xrack_assets::decode_png(&png)?;
        assert_eq!(image.size, [480, 300]);
        Ok((
            image,
            [
                after.hdr_resolves - before.hdr_resolves,
                after.bloom_passes - before.bloom_passes,
                after.bloom_pixels - before.bloom_pixels,
                after.color_draws - before.color_draws,
                after.bloom_target_allocations - before.bloom_target_allocations,
            ],
        ))
    };
    let halo = |before: &xrack_assets::Raster, after: &xrack_assets::Raster| -> usize {
        (70..150)
            .flat_map(|y| (264..325).map(move |x| 4 * (480 * y + x)))
            .filter(|&offset| after.rgba[offset].saturating_sub(before.rgba[offset]) >= 4)
            .count()
    };
    let same_view = |a: &xrack_assets::Raster, b: &xrack_assets::Raster| {
        a.rgba[..VIEW_BYTES] == b.rgba[..VIEW_BYTES]
    };
    let (plain, ordinary) = render(base)?;
    assert_eq!(
        &ordinary[..3],
        &[1, 0, 0],
        "HDR works without an environment"
    );
    let (ldr, ordinary_ldr) = render(View { hdr: false, ..base })?;
    assert_eq!(&ordinary_ldr[..3], &[0, 0, 0]);
    assert!(!same_view(&plain, &ldr));
    let (exposed, _) = render(View {
        exposure: 0.5,
        ..base
    })?;
    assert!(exposed.rgba[4 * (480 * 110 + 240)] < plain.rgba[4 * (480 * 110 + 240)]);

    let enabled = View { glow: true, ..base };
    let (bright, pyramid) = render(enabled)?;
    assert_eq!(
        pyramid,
        [1, 9, 79_200, ordinary[3], 5],
        "all bloom filtering is below full resolution"
    );
    let spread = halo(&plain, &bright);
    assert!(spread > 250, "bloom lights {spread} neighboring pixels");
    let (automatic, _) = render(View {
        hdr: false,
        ..enabled
    })?;
    assert!(
        same_view(&bright, &automatic),
        "bloom turns on HDR independently"
    );
    let (shallow, one_level) = render(View {
        levels: 1,
        ..enabled
    })?;
    assert_eq!(&one_level[..3], &[1, 1, 29_760]);
    assert!(
        halo(&plain, &shallow) < spread,
        "fewer levels narrow the halo"
    );

    let (limited, _) = render(View {
        threshold: 64.0,
        ..enabled
    })?;
    assert!(
        same_view(&plain, &limited),
        "the bright-pass threshold is scene-linear"
    );
    let (zero, skipped) = render(View {
        strength: 0.0,
        ..enabled
    })?;
    assert_eq!(&skipped[..3], &[1, 0, 0]);
    assert!(same_view(&plain, &zero));
    let (masked, _) = render(View {
        covered: true,
        ..base
    })?;
    let (occluded, _) = render(View {
        covered: true,
        ..enabled
    })?;
    assert!(
        same_view(&masked, &occluded),
        "an occluded source contributes no bloom"
    );

    let composite = View {
        glass: true,
        probe: true,
        ..base
    };
    let (clear_glass, _) = render(composite)?;
    let (glow_glass, combined) = render(View {
        glow: true,
        ..composite
    })?;
    assert_eq!(&combined[..3], &pyramid[..3]);
    assert!(halo(&clear_glass, &glow_glass) > 100);
    let (_, smaller) = render(View {
        width: 401.0,
        ..enabled
    })?;
    assert_eq!(smaller[4], 5);
    let (_, reused) = render(enabled)?;
    assert_eq!(
        reused[4], 0,
        "alternating viewport sizes reuses the bloom pyramid"
    );
    let (_, reused_smaller) = render(View {
        width: 401.0,
        ..enabled
    })?;
    assert_eq!(reused_smaller[4], 0);
    eprintln!(
        "Bloom filtered {} pixels and illuminated {spread} neighboring pixels in the test crop",
        pyramid[2]
    );
    Ok(())
}

#[test]
fn screen_space_occlusion_matches_backends_and_reuses_unchanged_depth() -> TestResult {
    const NAME: &str = "examples/puzzle3d/occlusion.xr";
    const ANALYSIS_PIXELS: u64 = 240 * 124;
    const VIEW_BYTES: usize = 480 * 248 * 4;
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("OcclusionDemo")?;
    let generated = occlusion_ui::OcclusionDemo::new();
    let weak = generated.as_weak();
    let mut dynamic_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut aot_view = PreviewScene::new(Box::new(generated));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy)]
    struct View {
        ao: bool,
        orthographic: bool,
        hdr: bool,
        glass: bool,
        strength: f64,
        radius: f64,
        ambient: f64,
        direct: f64,
        angle: f64,
    }
    let base = View {
        ao: false,
        orthographic: false,
        hdr: false,
        glass: false,
        strength: 0.95,
        radius: 1.35,
        ambient: 1.2,
        direct: 0.2,
        angle: 30.0,
    };
    let mut render = |view: View| -> TestResult<_> {
        let mut aot = weak.upgrade().ok_or("missing generated occlusion demo")?;
        aot.set_ao(view.ao);
        aot.set_orthographic(view.orthographic);
        aot.set_hdr(view.hdr);
        aot.set_glass(view.glass);
        aot.set_strength(view.strength);
        aot.set_radius(view.radius);
        aot.set_ambient(view.ambient);
        aot.set_direct(view.direct);
        aot.set_angle(view.angle);
        for (name, value) in [
            ("ao", Value::Bool(view.ao)),
            ("orthographic", Value::Bool(view.orthographic)),
            ("hdr", Value::Bool(view.hdr)),
            ("glass", Value::Bool(view.glass)),
            ("strength", Value::Float(view.strength)),
            ("radius", Value::Float(view.radius)),
            ("ambient", Value::Float(view.ambient)),
            ("direct", Value::Float(view.direct)),
            ("angle", Value::Float(view.angle)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot_view.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(
            png.as_slice() == dynamic_view.render(&mut renderer)?,
            "SSAO color pixels differ between AOT and interpreter"
        );
        let image = xrack_assets::decode_png(&png)?;
        assert_eq!(image.size, [480, 300]);
        Ok((
            image,
            [
                after.occlusion_depth_draws - before.occlusion_depth_draws,
                after.occlusion_evaluations - before.occlusion_evaluations,
                after.occlusion_pixels - before.occlusion_pixels,
            ],
        ))
    };
    let darkening = |previous: &xrack_assets::Raster, current: &xrack_assets::Raster| {
        let (from, _) = previous.rgba[..VIEW_BYTES].as_chunks::<4>();
        let (to, _) = current.rgba[..VIEW_BYTES].as_chunks::<4>();
        let delta = from
            .iter()
            .zip(to)
            .map(|(from, to)| from[0].saturating_sub(to[0]));
        delta.fold((0usize, 0u64), |(pixels, depth), value| {
            (pixels + usize::from(value >= 4), depth + u64::from(value))
        })
    };

    let (plain, off) = render(base)?;
    assert_eq!(off, [0, 0, 0]);
    let enabled = View { ao: true, ..base };
    let (occluded, first) = render(enabled)?;
    assert_eq!(
        first,
        [1, 1, ANALYSIS_PIXELS],
        "the opaque instances share one depth draw"
    );
    let (dark_pixels, total) = darkening(&plain, &occluded);
    assert!(
        dark_pixels > 250,
        "localized occlusion affects {dark_pixels} pixels"
    );
    assert_eq!(
        &plain.rgba[..4],
        &occluded.rgba[..4],
        "background remains unchanged"
    );
    let (lighter, strength_only) = render(View {
        strength: 0.5,
        ..enabled
    })?;
    assert_eq!(strength_only, [0, 0, 0]);
    let (_, lighter_total) = darkening(&plain, &lighter);
    assert!(lighter_total > 0 && lighter_total < total);

    let (_, light_only) = render(View {
        direct: 0.8,
        ..enabled
    })?;
    assert_eq!(light_only, [0, 0, 0]);
    let (narrow, radius_only) = render(View {
        radius: 0.65,
        ..enabled
    })?;
    assert_eq!(
        radius_only,
        [0, 1, ANALYSIS_PIXELS],
        "a new radius reuses camera depth"
    );
    assert!(darkening(&plain, &narrow).1 < total);
    let (_, camera_change) = render(View {
        angle: 46.0,
        ..enabled
    })?;
    assert_eq!(camera_change, first);

    let direct_only = View {
        ambient: 0.0,
        direct: 1.6,
        ..base
    };
    let (direct, _) = render(direct_only)?;
    let (direct_ao, no_ambient) = render(View {
        ao: true,
        ..direct_only
    })?;
    assert_eq!(no_ambient, [0, 0, 0]);
    assert!(direct.rgba[..VIEW_BYTES] == direct_ao.rgba[..VIEW_BYTES]);
    let (zero, bypassed) = render(View {
        strength: 0.0,
        ..enabled
    })?;
    assert_eq!(bypassed, [0, 0, 0]);
    assert!(plain.rgba[..VIEW_BYTES] == zero.rgba[..VIEW_BYTES]);

    let composite = View {
        orthographic: true,
        hdr: true,
        glass: true,
        ..base
    };
    let (unoccluded_hdr, _) = render(composite)?;
    let (occluded_hdr, hdr_stats) = render(View {
        ao: true,
        ..composite
    })?;
    assert_eq!(
        hdr_stats, first,
        "HDR/MSAA and transparent geometry share the viewport"
    );
    let (hdr_pixels, _) = darkening(&unoccluded_hdr, &occluded_hdr);
    assert!(
        hdr_pixels > 250,
        "orthographic occlusion affects {hdr_pixels} pixels"
    );
    eprintln!(
        "SSAO darkens {dark_pixels} perspective pixels and {hdr_pixels} orthographic HDR pixels; evaluates {ANALYSIS_PIXELS} half-resolution pixels"
    );
    Ok(())
}

#[test]
fn weighted_transparency_is_order_independent_and_reuses_opaque_depth() -> TestResult {
    const NAME: &str = "examples/puzzle3d/transparency.xr";
    const WIDTH: usize = 480;
    const VIEW_BYTES: usize = WIDTH * 248 * 4;
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("TransparencyDemo")?;
    let generated = transparency_ui::TransparencyDemo::new();
    let weak = generated.as_weak();
    let mut dynamic_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut aot_view = PreviewScene::new(Box::new(generated));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy, Default)]
    struct Frame {
        weighted: bool,
        reversed: bool,
        smooth: bool,
        hdr: bool,
        single: bool,
        offscreen: bool,
        red_depth_bias: f64,
        cache: bool,
    }
    let mut render = |frame: Frame| -> TestResult<_> {
        let mut aot = weak.upgrade().ok_or("missing transparency demo")?;
        aot.set_weighted(frame.weighted);
        aot.set_reversed(frame.reversed);
        aot.set_smooth(frame.smooth);
        aot.set_hdr(frame.hdr);
        aot.set_single(frame.single);
        aot.set_offscreen(frame.offscreen);
        aot.set_red_depth_bias(frame.red_depth_bias);
        aot.set_cache(frame.cache);
        interpreted.set("red-depth-bias", Value::Float(frame.red_depth_bias))?;
        interpreted.set("cache", Value::Bool(frame.cache))?;
        for (name, enabled) in [
            ("weighted", frame.weighted),
            ("reversed", frame.reversed),
            ("smooth", frame.smooth),
            ("hdr", frame.hdr),
            ("single", frame.single),
            ("offscreen", frame.offscreen),
        ] {
            interpreted.set(name, Value::Bool(enabled))?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot_view.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(png.as_slice() == dynamic_view.render(&mut renderer)?);
        assert_eq!(
            after.transparency_composites - before.transparency_composites,
            u64::from(frame.weighted && !frame.offscreen)
        );
        assert_eq!(
            after.multisample_resolves - before.multisample_resolves,
            u64::from(frame.smooth)
        );
        let picks = if frame.offscreen {
            ["", ""]
        } else if frame.single {
            ["red", "red"]
        } else {
            ["red", "blue"]
        };
        assert_eq!(
            [aot.get_left_pick().as_str(), aot.get_right_pick().as_str()],
            picks
        );
        for (name, expected) in ["left-pick", "right-pick"].into_iter().zip(picks) {
            assert_eq!(interpreted.get(name)?, Value::String(expected.into()));
        }
        Ok((
            xrack_assets::decode_png(&png)?,
            [
                after.color_draws - before.color_draws,
                after.color_instances - before.color_instances,
                after.culled_instances - before.culled_instances,
            ],
            [
                renderer.scene_mesh_uploads(),
                renderer.scene_instance_uploads(),
            ],
        ))
    };
    let pixel = |image: &xrack_assets::Raster, x, y| -> [u8; 4] {
        let offset = 4 * (WIDTH * y + x);
        std::array::from_fn(|index| image.rgba[offset + index])
    };
    let difference = |a: &xrack_assets::Raster, b: &xrack_assets::Raster| -> u8 {
        a.rgba[..VIEW_BYTES]
            .iter()
            .zip(&b.rgba[..VIEW_BYTES])
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or_default()
    };
    let base = Frame::default();
    let (sorted, sorted_counts, _) = render(base)?;
    let (sorted_reverse, _, _) = render(Frame {
        reversed: true,
        ..base
    })?;
    assert_eq!(sorted.size, [480, 344]);
    assert!(
        difference(&sorted, &sorted_reverse) > 20,
        "submesh ties follow submission order"
    );

    let (near, _, near_uploads) = render(Frame {
        red_depth_bias: -0.8,
        cache: true,
        ..base
    })?;
    let (far, _, far_uploads) = render(Frame {
        red_depth_bias: 0.8,
        cache: true,
        ..base
    })?;
    // Measure where only the red and blue sheets overlap.
    let near_pixel = pixel(&near, 185, 98);
    let far_pixel = pixel(&far, 185, 98);
    assert!(near_pixel[0].saturating_sub(far_pixel[0]) > 20);
    assert!(far_pixel[2].saturating_sub(near_pixel[2]) > 20);
    assert_eq!(near_uploads, far_uploads);
    let (far_reordered, _, _) = render(Frame {
        red_depth_bias: 0.8,
        cache: true,
        reversed: true,
        ..base
    })?;
    assert_eq!(difference(&far, &far_reordered), 0);

    let enabled = Frame {
        weighted: true,
        ..base
    };
    let (weighted, batches, _) = render(enabled)?;
    let (weighted_reverse, reverse_batches, _) = render(Frame {
        reversed: true,
        ..enabled
    })?;
    assert_eq!(sorted_counts, [4, 4, 1]);
    assert_eq!(
        batches,
        [3, 4, 1],
        "the two visible sheets share one instanced draw"
    );
    assert_eq!(batches, reverse_batches);
    let reorder_error = difference(&weighted, &weighted_reverse);
    assert!(
        reorder_error <= 1,
        "reordering changes weighted pixels by {reorder_error}"
    );
    let (weighted_bias, biased_batches, _) = render(Frame {
        red_depth_bias: -0.8,
        ..enabled
    })?;
    assert!(difference(&weighted, &weighted_bias) <= 1);
    assert_eq!(biased_batches, batches);
    let left = pixel(&weighted, 185, 98);
    let right = pixel(&weighted, 295, 98);
    assert!(
        left[0] > left[2] && right[2] > right[0],
        "per-pixel depth: {left:?} and {right:?}"
    );

    let (occluder_only, _, _) = render(Frame {
        offscreen: true,
        ..enabled
    })?;
    assert_eq!(pixel(&weighted, 240, 150), pixel(&occluder_only, 240, 150));
    let hd = Frame {
        smooth: true,
        hdr: true,
        ..enabled
    };
    let (hdr, _, _) = render(hd)?;
    let (hdr_reverse, _, _) = render(Frame {
        reversed: true,
        ..hd
    })?;
    let hdr_error = difference(&hdr, &hdr_reverse);
    assert!(hdr_error <= 1, "HDR/MSAA reorder difference: {hdr_error}");

    let mut msaa_errors = [0, 0];
    for (hdr, error) in [false, true].into_iter().zip(&mut msaa_errors) {
        let single = Frame {
            hdr,
            single: true,
            ..hd
        };
        let (single_weighted, _, _) = render(single)?;
        let (single_sorted, _, _) = render(Frame {
            weighted: false,
            ..single
        })?;
        *error = difference(&single_weighted, &single_sorted);
        assert!(
            *error <= 2,
            "one layer per MSAA sample follows source-over: {error}, HDR={hdr}"
        );
    }
    eprintln!(
        "Weighted OIT batched {sorted_counts:?} → {batches:?}; reordering error {reorder_error}, HDR/MSAA {hdr_error}, LDR/HDR MSAA edges {msaa_errors:?}"
    );
    Ok(())
}

#[test]
fn clearcoat_and_emission_match_in_aot_and_interpreted_materials() -> TestResult {
    const NAME: &str = "examples/puzzle3d/materials.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for file in ["coat-channels.png", "coat-normal.png"] {
        assert!(inputs.iter().any(|path| path.ends_with(file)), "{file}");
    }
    let baked = xrack_compiler::compile_preview(&source, NAME, Default::default())?;
    assert_eq!(
        baked
            .rust
            .to_string()
            .matches("MeshImage :: from_packed")
            .count(),
        2
    );

    let dynamic = program.create("MaterialDemo")?;
    let app = material_ui::MaterialDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    let mut render = |hdr, shadows, yaw, filtered| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing material demo")?;
        app.set_hdr(hdr);
        app.set_shadows(shadows);
        app.set_light_yaw(yaw);
        app.set_filtered(filtered);
        for (name, value) in [
            ("hdr", Value::Bool(hdr)),
            ("shadows", Value::Bool(shadows)),
            ("light-yaw", Value::Float(yaw)),
            ("filtered", Value::Bool(filtered)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let pixel_data = generated.render(&mut renderer)?.to_vec();
        let draws = renderer.scene_draw_stats().color_draws - before.color_draws;
        assert!(pixel_data.as_slice() == interpreted.render(&mut renderer)?);
        assert_eq!(app.get_picked(), "finish");
        assert_eq!(dynamic.get("picked")?, Value::String("finish".into()));
        Ok((
            xrack_assets::decode_png(&pixel_data)?,
            draws,
            renderer.scene_mesh_uploads(),
            renderer.scene_shadow_passes(),
        ))
    };

    let sample = |image: &xrack_assets::Raster, x: usize, y: usize| image.rgba[4 * (480 * y + x)];
    let (direct, draws, meshes, shadow_passes) = render(false, false, 0.0, false)?;
    assert_eq!(direct.size, [480, 300]);
    assert_eq!(draws, 6, "one color draw per material section");
    let matte = sample(&direct, 148, 82);
    let clearcoat = sample(&direct, 240, 82);
    let mask_left = sample(&direct, 313, 67);
    let mask_right = sample(&direct, 350, 67);
    let mask_bottom = sample(&direct, 313, 99);
    assert!(clearcoat > matte + 35, "matte={matte}, coat={clearcoat}");
    assert!(
        mask_left > mask_right + 5,
        "R controls coat amount: R=1 {mask_left}, R=0 {mask_right}"
    );
    assert_ne!(mask_bottom, mask_left, "G controls coat roughness");

    let (hdr, hdr_draws, hdr_meshes, _) = render(true, false, 0.0, false)?;
    assert_eq!(hdr_draws, draws);
    assert_eq!(hdr_meshes, meshes);
    let regular = sample(&hdr, 148, 166);
    let emissive = sample(&hdr, 240, 166);
    assert!(
        emissive > regular + 35,
        "core={regular}, extended={emissive}"
    );

    let (from_left, _, left_meshes, _) = render(false, false, -40.0, false)?;
    let (from_right, _, right_meshes, _) = render(false, false, 40.0, false)?;
    let (_, shadowed_draws, shadow_meshes, next_shadows) = render(false, true, 40.0, false)?;
    assert_eq!(left_meshes, meshes);
    assert_eq!(right_meshes, meshes);
    assert_eq!(shadow_meshes, meshes);
    assert_eq!(shadowed_draws, draws);
    assert!(
        next_shadows > shadow_passes,
        "coat and masked shadows coexist"
    );
    let mapped = [sample(&from_left, 332, 165), sample(&from_right, 332, 165)];
    assert!(
        i16::from(mapped[0]) - i16::from(mapped[1]) > 18,
        "the mirrored coat coordinates reflect the light from the left: {mapped:?}"
    );

    let (filtered_normal, filtered_draws, filtered_meshes, _) = render(false, false, -40.0, true)?;
    assert_eq!(filtered_draws, draws);
    assert_eq!(filtered_meshes, meshes);
    assert_eq!(
        sample(&filtered_normal, 148, 82),
        sample(&from_left, 148, 82)
    );
    let mapped_changes = (139..190)
        .flat_map(|y| (298..365).map(move |x| (x, y)))
        .filter(|&(x, y)| sample(&filtered_normal, x, y) != sample(&from_left, x, y))
        .count();
    assert!(
        mapped_changes > 0,
        "the alpha-masked coat normal map participates in AA"
    );
    eprintln!("specular AA changed {mapped_changes} mapped clearcoat pixels");

    for scene in [&generated, &interpreted] {
        let (node, _) = inspect(scene)?;
        let Some(Value::SceneObjects(objects)) = node.properties.get("model") else {
            return Err("missing prepared clearcoat model".into());
        };
        let materials = objects[0].mesh.materials();
        assert_eq!(materials.len(), 6);
        let pbr = materials[5].pbr.as_ref().ok_or("missing PBR material")?;
        let base = pbr.normal_map.as_ref().ok_or("missing base normal")?;
        let coat = pbr.clearcoat.as_ref().ok_or("missing clearcoat")?;
        let normal = coat.normal_map.as_ref().ok_or("missing clearcoat normal")?;
        assert_eq!([base.texture.uv_slot, normal.texture.uv_slot], [0, 1]);
        assert!(std::sync::Arc::ptr_eq(
            &base.texture.image,
            &normal.texture.image
        ));
        let emissive = materials[4]
            .pbr
            .as_ref()
            .ok_or("missing emission")?
            .emissive;
        assert!((emissive[0] - 1.7).abs() < 0.001);
    }
    eprintln!(
        "Clearcoat swatch red channels (matte / coated / masked R=1 / R=0): {matte} / {clearcoat} / {mask_left} / {mask_right}; HDR emission {regular} / {emissive}; shifted normal {mapped:?}"
    );
    Ok(())
}

#[test]
fn gltf_cameras_preserve_framing_rays_and_infinite_far_in_both_backends() -> TestResult {
    use xrack_runtime::{Length, Point2D, scene3d::Point3D};

    const NAME: &str = "examples/puzzle3d/imported_cameras.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ImportedCamerasDemo")?;
    let app = imported_cameras_ui::ImportedCamerasDemo::new();
    let projected = app.get_projected();
    // The cold camera-only query runs before either viewport builds geometry.
    assert!(projected.valid && projected.inside);
    assert_eq!(
        dynamic.get("projected")?,
        Value::ProjectedPoint3D(projected)
    );
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-3, "{a} != {b}");
    near(projected.point.x.0, 240.0 + 240.0 * 3.0_f64.sqrt() / 6.0);
    near(projected.depth, 6.0);
    let round_trip = app.get_round_trip();
    near(round_trip.x, 2.0);
    near(round_trip.y, 0.0);
    near(round_trip.z, 0.0);
    assert_eq!(dynamic.get("round-trip")?, Value::Point3D(round_trip));
    assert_eq!(app.get_picked(), "subject");
    assert_eq!(dynamic.get("picked")?, Value::String("subject".into()));

    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut render = |camera: &str, time, effects, x| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing camera demo")?;
        let cursor = Point2D::new(Length(x), Length(120.0));
        app.set_selected_camera(camera.into());
        app.set_time(time);
        app.set_effects(effects);
        app.set_cursor(cursor);
        for (property, value) in [
            ("selected-camera", Value::String(camera.into())),
            ("time", Value::Float(time)),
            ("effects", Value::Bool(effects)),
            ("cursor", Value::Point2D(cursor)),
        ] {
            dynamic.set(property, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        assert_eq!(dynamic.get("picked")?, Value::String(app.get_picked()));
        assert_eq!(
            dynamic.get("projected")?,
            Value::ProjectedPoint3D(app.get_projected())
        );
        assert_eq!(
            after.depth_of_field_passes - before.depth_of_field_passes,
            if effects { 3 } else { 0 }
        );
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing camera rig scene".into());
        };
        Ok((
            xrack_assets::decode_png(&png)?,
            scene.clone(),
            app.get_picked(),
            app.get_projected(),
        ))
    };

    let (hero, scene, bar_hit, _) = render("rig/Hero", 0.0, false, 35.0)?;
    let frames = [480.0, 240.0];
    let authored = scene
        .camera("rig/Hero", frames)
        .ok_or("missing perspective camera")?;
    let indexed = scene
        .camera("rig/#1", frames)
        .ok_or("missing indexed camera")?;
    assert!(authored == indexed);
    assert_eq!(authored.content_rect(), [120.0, 0.0, 240.0, 240.0]);
    assert!(scene.camera("rig/Duplicate", frames).is_none());
    assert!(scene.camera("rig/#4", frames).is_some());
    assert!(scene.camera("rig/#5", frames).is_some());
    assert!(bar_hit.is_empty());
    let pixel = |frame: &xrack_assets::Raster, x: usize, y: usize| {
        let start = 4 * (480 * y + x);
        [
            frame.rgba[start],
            frame.rgba[start + 1],
            frame.rgba[start + 2],
        ]
    };
    assert_eq!(pixel(&hero, 35, 120), [0x17, 0x23, 0x33]);
    let (moved, moving_scene, _, dolly) = render("rig/Hero", 2.0, false, 240.0)?;
    assert_ne!(hero.rgba, moved.rgba);
    near(dolly.point.x.0, 240.0 + 120.0 * 3.0_f64.sqrt() * 0.4 / 6.0);
    near(dolly.depth, 6.0);
    let eye = moving_scene
        .camera("rig/Hero", frames)
        .ok_or("missing dolly")?
        .eye_position();
    near(f64::from(eye[0]), 1.6);

    let (_, orthographic, hit, map_marker) = render("rig/Map", 2.0, true, 240.0)?;
    let map = orthographic
        .camera("rig/Map", frames)
        .ok_or("missing ortho")?;
    assert_eq!(map.content_rect(), [0.0, 0.0, 480.0, 240.0]);
    assert_eq!(map.depth_limits(), [0.0, 34.0]);
    assert_eq!(hit, "subject");
    near(map_marker.point.x.0, 480.0);

    let (unbounded, far_scene, far_hit, _) = render("rig/Far", 0.0, true, 301.0)?;
    let far = far_scene
        .camera("rig/Far", frames)
        .ok_or("missing infinite perspective")?;
    assert!(far.depth_limits()[1].is_infinite());
    let distant = far.project(Point3D::new(32.0, 0.0, -155.0));
    assert!(distant.valid && distant.inside);
    assert_eq!(far_hit, "far-item");
    assert!(pixel(&unbounded, 301, 120)[2] > pixel(&unbounded, 301, 120)[0]);
    let (_, bounded, clipped, _) = render("rig/Hero", 0.0, false, 301.0)?;
    let out = bounded
        .camera("rig/Hero", frames)
        .ok_or("missing finite camera")?
        .project(Point3D::new(32.0, 0.0, -155.0));
    assert!(out.valid && !out.inside);
    assert!(clipped.is_empty());
    eprintln!(
        "glTF camera dolly x={}; far target at depth {} with an infinite projection",
        eye[0], distant.depth
    );
    Ok(())
}

#[test]
fn failed_probe_slice_preserves_the_published_cube_and_restarts_capture() -> TestResult {
    use live_reflections_ui::LiveReflectionsDemo;
    use xrack_layout::Interaction;
    use xrack_runtime::Length;

    let mut app = LiveReflectionsDemo::new();
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    let warm = gpu.snapshot(&app.render(), 532, 382, 1.0, &input)?;
    let viewport_bytes = 4 * 532 * 270;
    assert_eq!(gpu.scene_draw_stats().reflection_cubemap_updates, 1);
    app.set_slicing("individual-faces".into());
    app.set_warm(false);
    let scene = app.render();
    let started = gpu.snapshot(&scene, 532, 382, 1.0, &input)?;
    assert!(started.rgba[..viewport_bytes] == warm.rgba[..viewport_bytes]);

    let registration =
        xrack_renderer_femtovg::register_gpu("abort-probe-slice", |_| Err("bad slice".into()))?;
    let mut broken = Node::new("CustomRender", Some("abort"));
    broken
        .properties
        .insert("renderer", Value::String("abort-probe-slice".into()));
    broken
        .properties
        .insert("width", Value::Length(Length(24.0)));
    broken
        .properties
        .insert("height", Value::Length(Length(24.0)));
    broken.properties.insert("z", Value::Float(200.0));
    let mut failing = scene.clone();
    failing.children.push(broken);
    let before = gpu.scene_draw_stats();
    let error = gpu
        .snapshot(&failing, 532, 382, 1.0, &input)
        .err()
        .ok_or("expected frame failure")?;
    assert!(error.contains("bad slice"));
    assert_eq!(
        gpu.scene_draw_stats().reflection_capture_faces - before.reflection_capture_faces,
        1
    );
    drop(registration);

    for step in 0..12 {
        let frame = gpu.snapshot(&scene, 532, 382, 1.0, &input)?;
        let published = step == 11;
        assert_eq!(
            gpu.scene_draw_stats().reflection_cubemap_updates,
            if published { 2 } else { 1 }
        );
        assert_eq!(
            frame.rgba[..viewport_bytes] == warm.rgba[..viewport_bytes],
            !published
        );
    }
    assert!(!gpu.needs_redraw());
    Ok(())
}

#[test]
fn sliced_probe_updates_publish_coherent_cubes_without_starving_animated_inputs() -> TestResult {
    use live_reflections_ui::LiveReflectionsDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/live_reflections.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("LiveReflectionsDemo")?;
    let app = LiveReflectionsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;
    let work = |before: SceneDrawStats, after: SceneDrawStats| {
        [
            after.reflection_capture_faces - before.reflection_capture_faces,
            after.reflection_prefilter_passes - before.reflection_prefilter_passes,
            after.reflection_cubemap_updates - before.reflection_cubemap_updates,
        ]
    };
    struct Frame {
        work: [u64; 3],
        image: Vec<u8>,
        wakes: bool,
    }
    let mut draw = |slicing: &str, warm: bool| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing probe demo")?;
        app.set_slicing(slicing.into());
        app.set_warm(warm);
        dynamic.set("slicing", Value::String(slicing.into()))?;
        dynamic.set("warm", Value::Bool(warm))?;
        let before = gpu.scene_draw_stats();
        let preview_before = preview_gpu.scene_draw_stats();
        let png = aot.render_animated(&mut gpu)?.to_vec();
        assert!(
            png.as_slice() == interpreted.render_animated(&mut preview_gpu)?,
            "{slicing} {warm}"
        );
        let actual = work(before, gpu.scene_draw_stats());
        assert_eq!(actual, work(preview_before, preview_gpu.scene_draw_stats()));
        let wakes = aot.next_frame_delay().is_some();
        assert_eq!(wakes, gpu.needs_redraw());
        assert_eq!(wakes, interpreted.next_frame_delay().is_some());
        let image = xrack_assets::decode_png(&png)?;
        Ok(Frame {
            work: actual,
            image: image.rgba[..4 * 532 * 270].to_vec(),
            wakes,
        })
    };
    let cold = draw("none", false)?;
    let warm = draw("none", true)?;
    assert_eq!(cold.work, [6, 36, 1]);
    assert_eq!(warm.work, [6, 36, 1]);
    assert!(cold.image != warm.image);
    assert!(!warm.wakes);

    // Start with the cold inputs. Changing the model every later frame must
    // neither restart the current cube nor mix colors across its faces.
    for step in 0..12 {
        let frame = draw("individual-faces", step % 2 == 1)?;
        let expected = if step < 6 {
            [1, 0, 0]
        } else {
            [0, 6, u64::from(step == 11)]
        };
        assert_eq!(frame.work, expected, "capture step {step}");
        assert!(
            frame.wakes,
            "pending work or a newer scene must request another frame"
        );
        assert!(
            frame.image == if step == 11 { &cold } else { &warm }.image,
            "capture step {step}"
        );
    }
    // The final animation state is warm and requires another capture, even
    // though the application has stopped mutating the model.
    for step in 0..12 {
        let frame = draw("individual-faces", true)?;
        assert_eq!(
            frame.work,
            if step < 6 {
                [1, 0, 0]
            } else {
                [0, 6, u64::from(step == 11)]
            }
        );
        assert_eq!(frame.wakes, step < 11);
        assert!(frame.image == if step == 11 { &warm } else { &cold }.image);
    }
    assert_eq!(draw("individual-faces", true)?.work, [0; 3]);

    for step in 0..7 {
        let frame = draw("all-faces-at-once", false)?;
        assert_eq!(
            frame.work,
            if step == 0 {
                [6, 0, 0]
            } else {
                [0, 6, u64::from(step == 6)]
            }
        );
        assert_eq!(frame.wakes, step < 6);
        assert!(
            frame.image == if step == 6 { &cold } else { &warm }.image,
            "mip step {step}"
        );
    }
    let idle = draw("all-faces-at-once", false)?;
    assert_eq!(idle.work, [0; 3]);
    assert!(!idle.wakes);
    Ok(())
}

#[test]
fn live_probe_captures_scene_color_and_reuses_completed_cubemaps() -> TestResult {
    use live_reflections_ui::LiveReflectionsDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/live_reflections.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("LiveReflectionsDemo")?;
    let app = LiveReflectionsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;
    #[derive(Clone, Copy, Debug)]
    struct Settings {
        mode: &'static str,
        warm: bool,
        casts: bool,
        receives: bool,
        orbit: f64,
        probe_x: f64,
        revision: i64,
    }
    struct Frame {
        image: Vec<u8>,
        center: [u8; 4],
        work: [u64; 5],
        uploads: [u64; 2],
    }
    let delta = |before: SceneDrawStats, after: SceneDrawStats| {
        [
            after.reflection_cubemap_updates - before.reflection_cubemap_updates,
            after.reflection_capture_faces - before.reflection_capture_faces,
            after.reflection_prefilter_passes - before.reflection_prefilter_passes,
            after.viewport_renders - before.viewport_renders,
            after.viewport_cache_hits - before.viewport_cache_hits,
        ]
    };
    let mut draw = |options: Settings| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing probe demo")?;
        app.set_refresh(options.mode.into());
        app.set_warm(options.warm);
        app.set_casts(options.casts);
        app.set_receives(options.receives);
        app.set_orbit(options.orbit);
        app.set_probe_x(options.probe_x);
        app.set_revision(options.revision);
        for (name, value) in [
            ("refresh", Value::String(options.mode.into())),
            ("warm", Value::Bool(options.warm)),
            ("casts", Value::Bool(options.casts)),
            ("receives", Value::Bool(options.receives)),
            ("orbit", Value::Float(options.orbit)),
            ("probe-x", Value::Float(options.probe_x)),
            ("revision", Value::Int(options.revision)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let dynamic_before = preview_gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png.as_slice() == interpreted.render(&mut preview_gpu)?,
            "{options:?}"
        );
        let work = delta(before, gpu.scene_draw_stats());
        assert_eq!(work, delta(dynamic_before, preview_gpu.scene_draw_stats()));
        assert_eq!(
            gpu.scene_environment_uploads(),
            0,
            "live cubemaps stay on the GPU"
        );
        let image = xrack_assets::decode_png(&png)?;
        let index = 4 * (135 * 532 + 266);
        let center = std::array::from_fn(|channel| image.rgba[index + channel]);
        Ok(Frame {
            image: image.rgba,
            center,
            work,
            uploads: [gpu.scene_mesh_uploads(), gpu.scene_instance_uploads()],
        })
    };
    let base = Settings {
        mode: "on-change",
        warm: true,
        casts: true,
        receives: true,
        orbit: 0.0,
        probe_x: 0.0,
        revision: 0,
    };
    let warm = draw(base)?;
    let captured = [1, 6, 36, 1, 0];
    assert_eq!(warm.work, captured);
    let retained = draw(base)?;
    assert_eq!(retained.work, [0, 0, 0, 0, 1]);
    assert!(retained.image == warm.image);
    let orbit = draw(Settings { orbit: 8.0, ..base })?;
    assert_eq!(orbit.work, [0, 0, 0, 1, 0]);
    assert_eq!(orbit.uploads, warm.uploads);
    let cold = draw(Settings {
        warm: false,
        ..base
    })?;
    assert_eq!(cold.work, captured);
    let [wr, _, wb, _] = warm.center;
    let [cr, _, cb, _] = cold.center;
    assert!(
        wr > wb.saturating_add(12) && cb > cr.saturating_add(12),
        "{wr}/{wb}, {cr}/{cb}"
    );

    let excluded = draw(Settings {
        casts: false,
        ..base
    })?;
    assert_eq!(excluded.work, captured);
    assert!(excluded.center[0] < wr.saturating_sub(12));
    let opted_out = draw(Settings {
        receives: false,
        ..base
    })?;
    assert_eq!(opted_out.work, [0, 0, 0, 1, 0]);
    assert_ne!(opted_out.center, warm.center);

    let first = Settings {
        mode: "first-frame",
        ..base
    };
    let fixed = draw(first)?;
    assert_eq!(fixed.work, captured);
    let frozen = draw(Settings {
        warm: false,
        ..first
    })?;
    assert_eq!(frozen.work[..3], [0; 3]);
    assert_eq!(frozen.center, fixed.center);
    let refreshed = draw(Settings {
        warm: false,
        revision: 1,
        ..first
    })?;
    assert_eq!(refreshed.work, captured);
    assert_eq!(refreshed.center, cold.center);
    let every = Settings {
        mode: "every-frame",
        ..base
    };
    assert_eq!(draw(every)?.work, captured);
    assert_eq!(draw(every)?.work, captured);
    assert_eq!(
        draw(Settings {
            probe_x: 1.0,
            ..base
        })?
        .work,
        captured
    );
    Ok(())
}

#[test]
fn viewport_cache_reuses_completed_color_across_2d_frames_and_compares_both_backends() -> TestResult
{
    use viewport_cache_ui::ViewportCacheDemo;
    use xrack_runtime::{Color, Length};

    const NAME: &str = "examples/puzzle3d/viewport_cache.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ViewportCacheDemo")?;
    let app = ViewportCacheDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;
    #[derive(Clone, Copy, Debug)]
    struct Settings {
        cache: bool,
        reference: bool,
        tick: i64,
        angle: f64,
        light: f64,
        color: Color,
        offset: f64,
        opacity: f64,
        pixels: i64,
        effects: bool,
        dpi: f32,
    }
    struct Frame {
        image: xrack_assets::Raster,
        work: [u64; 3],
        allocations: u64,
        shadow_passes: u64,
        effects: [u64; 2],
    }
    let mut draw = |frame: Settings| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing viewport cache demo")?;
        app.set_use_cache(frame.cache);
        app.set_reference(frame.reference);
        app.set_tick(frame.tick);
        app.set_angle(frame.angle);
        app.set_light_angle(frame.light);
        app.set_ball_color(frame.color);
        app.set_offset(Length(frame.offset));
        app.set_scene_alpha(frame.opacity);
        app.set_fixed_size(frame.pixels);
        app.set_effects(frame.effects);
        for (name, value) in [
            ("use-cache", Value::Bool(frame.cache)),
            ("reference", Value::Bool(frame.reference)),
            ("tick", Value::Int(frame.tick)),
            ("angle", Value::Float(frame.angle)),
            ("light-angle", Value::Float(frame.light)),
            ("ball-color", Value::Color(frame.color)),
            ("offset", Value::Length(Length(frame.offset))),
            ("scene-alpha", Value::Float(frame.opacity)),
            ("fixed-size", Value::Int(frame.pixels)),
            ("effects", Value::Bool(frame.effects)),
        ] {
            dynamic.set(name, value)?;
        }
        let prior = gpu.scene_draw_stats();
        let png = aot.render_scaled(&mut gpu, frame.dpi)?.to_vec();
        let after = gpu.scene_draw_stats();
        assert!(
            png.as_slice() == interpreted.render_scaled(&mut preview_gpu, frame.dpi)?,
            "{frame:?}"
        );
        Ok(Frame {
            image: xrack_assets::decode_png(&png)?,
            work: [
                after.viewport_renders - prior.viewport_renders,
                after.viewport_cache_hits - prior.viewport_cache_hits,
                after.raster_pixels - prior.raster_pixels,
            ],
            allocations: after.viewport_cache_allocations,
            shadow_passes: gpu.scene_shadow_passes(),
            effects: [after.bloom_passes, after.depth_of_field_passes],
        })
    };
    let base = Settings {
        cache: true,
        reference: false,
        tick: 0,
        angle: 0.0,
        light: -34.0,
        color: Color(0xe8b158ff),
        offset: 0.0,
        opacity: 1.0,
        pixels: 0,
        effects: true,
        dpi: 1.0,
    };
    let first = draw(base)?;
    assert_eq!(first.work, [1, 0, 256 * 202]);
    assert_eq!(first.allocations, 1);
    assert!(first.shadow_passes > 0 && first.effects.into_iter().all(|passes| passes > 0));
    let count = draw(Settings { tick: 1, ..base })?;
    assert_eq!(count.work, [0, 1, 0]);
    assert_eq!(count.shadow_passes, first.shadow_passes);
    assert_eq!(count.effects, first.effects);
    let view_bytes = 552 * 236 * 4;
    assert!(count.image.rgba[..view_bytes] == first.image.rgba[..view_bytes]);
    assert!(count.image.rgba[view_bytes..] != first.image.rgba[view_bytes..]);

    let moved = draw(Settings {
        offset: 7.0,
        opacity: 0.7,
        ..base
    })?;
    assert_eq!(moved.work, [0, 1, 0]);
    assert!(moved.image.rgba[..view_bytes] != first.image.rgba[..view_bytes]);
    let both = Settings {
        reference: true,
        ..base
    };
    let shared = draw(both)?;
    assert_eq!(shared.work, [0, 2, 0]);
    let rotated = draw(Settings {
        angle: 25.0,
        ..both
    })?;
    assert_eq!(rotated.work, [1, 1, 256 * 202]);
    assert_eq!(
        rotated.allocations, 2,
        "the sampled texture stays intact for the other view"
    );
    let right_pixels = |frame: &Frame| -> Vec<u8> {
        (34..236)
            .flat_map(|y| {
                frame.image.rgba[4 * (552 * y + 284)..4 * (552 * y + 540)]
                    .iter()
                    .copied()
            })
            .collect()
    };
    assert!(right_pixels(&rotated) == right_pixels(&shared));
    assert!(rotated.image.rgba[..view_bytes] != shared.image.rgba[..view_bytes]);

    let relit = draw(Settings {
        light: -55.0,
        ..base
    })?;
    assert_eq!(relit.work, [1, 0, 256 * 202]);
    assert_eq!(relit.allocations, rotated.allocations);
    let recolored = draw(Settings {
        color: Color(0xc7658aff),
        light: -55.0,
        ..base
    })?;
    assert_eq!(recolored.work, [1, 0, 256 * 202]);
    assert!(recolored.image.rgba[..view_bytes] != relit.image.rgba[..view_bytes]);
    let without_effects = draw(Settings {
        effects: false,
        ..base
    })?;
    assert_eq!(without_effects.work, [1, 0, 256 * 202]);
    assert!(without_effects.image.rgba[..view_bytes] != first.image.rgba[..view_bytes]);

    let fixed = Settings {
        pixels: 128,
        ..base
    };
    let fixed_first = draw(fixed)?;
    assert_eq!(fixed_first.work, [1, 0, 128 * 128]);
    let retina = draw(Settings { dpi: 2.0, ..fixed })?;
    assert_eq!(retina.image.size, [1104, 620]);
    assert_eq!(retina.work, [0, 1, 0]);
    let uncached = draw(Settings {
        cache: false,
        ..fixed
    })?;
    assert_eq!(uncached.work, [1, 0, 128 * 128]);
    assert!(uncached.image.rgba[..view_bytes] == fixed_first.image.rgba[..view_bytes]);
    Ok(())
}

#[test]
fn a_failed_frame_preserves_only_completed_viewport_content() -> TestResult {
    use viewport_cache_ui::ViewportCacheDemo;
    use xrack_layout::Interaction;
    use xrack_runtime::Length;

    let source = xrack_renderer_femtovg::register_gpu("viewport-cache-abort", |_| {
        Err("frame failure".into())
    })?;
    let mut broken = Node::new("CustomRender", Some("abort"));
    broken
        .properties
        .insert("renderer", Value::String("viewport-cache-abort".into()));
    broken
        .properties
        .insert("width", Value::Length(Length(24.0)));
    broken
        .properties
        .insert("height", Value::Length(Length(24.0)));
    broken.properties.insert("z", Value::Float(200.0));
    let mut app = ViewportCacheDemo::new();
    app.set_reference(false);
    let scene = app.render();
    let mut failing = scene.clone();
    failing.children.push(broken);
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let input = Interaction::default();
    let error = gpu
        .snapshot(&failing, 552, 310, 1.0, &input)
        .err()
        .ok_or("expected failed frame")?;
    assert!(error.contains("frame failure"));
    assert_eq!(gpu.scene_draw_stats().viewport_renders, 1);
    drop(source);
    let recovery = gpu.snapshot(&scene, 552, 310, 1.0, &input)?;
    assert_eq!(gpu.scene_draw_stats().viewport_renders, 2);
    assert_eq!(gpu.scene_draw_stats().viewport_cache_hits, 0);
    let reused = gpu.snapshot(&scene, 552, 310, 1.0, &input)?;
    assert_eq!(gpu.scene_draw_stats().viewport_renders, 2);
    assert_eq!(gpu.scene_draw_stats().viewport_cache_hits, 1);
    let fresh = renderer()?.snapshot(&scene, 552, 310, 1.0, &input)?;
    assert!(fresh.rgba == recovery.rgba && fresh.rgba == reused.rgba);
    Ok(())
}

#[test]
fn content_layers_share_model_buffers_across_views_and_filter_shadows_and_picks() -> TestResult {
    use content_layers_ui::ContentLayersDemo;
    use xrack_runtime::scene3d::Hit3D;

    const NAME: &str = "examples/puzzle3d/content_layers.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ContentLayersDemo")?;
    let app = ContentLayersDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;
    struct Frame {
        image: xrack_assets::Raster,
        submitted: u64,
        layers: u64,
        clipped: u64,
        shadow_passes: u64,
        uploads: [u64; 2],
    }
    let mut draw = |mask, hidden_x, culling, expected: &[&str]| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing layer demo")?;
        app.set_mask(mask);
        app.set_hidden_x(hidden_x);
        app.set_culling(culling);
        dynamic.set("mask", Value::Int(mask))?;
        dynamic.set("hidden-x", Value::Float(hidden_x))?;
        dynamic.set("culling", Value::Bool(culling))?;
        let first = expected.first().copied().unwrap_or_default();
        assert_eq!(app.get_picked(), first);
        assert_eq!(dynamic.get("picked")?, Value::String(first.into()));
        assert_eq!(app.get_map_picked(), "objective");
        assert_eq!(
            dynamic.get("map-picked")?,
            Value::String("objective".into())
        );
        for (name, hit, id) in [
            ("hit", app.get_hit(), first),
            ("ray-hit", app.get_ray_hit(), first),
            ("map-hit", app.get_map_hit(), "objective"),
        ] {
            assert_eq!((hit.valid, hit.object_id.as_str()), (!id.is_empty(), id));
            assert_eq!(dynamic.get(name)?, Value::Hit3D(hit));
        }
        let ids = |hits: &VecModel<Hit3D>| {
            (0..hits.len())
                .filter_map(|i| hits.row(i))
                .map(|hit| hit.object_id.clone())
                .collect::<Vec<_>>()
        };
        for (name, hits) in [
            ("hits", app.get_hits()),
            ("map-hits", app.get_map_hits()),
            ("ray-hits", app.get_ray_hits()),
        ] {
            assert_eq!(
                hits.iter()
                    .map(|hit| hit.object_id.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
            let Value::Hit3DList(interpreted_hits) = dynamic.get(name)? else {
                return Err("expected scene hit list".into());
            };
            assert_eq!(ids(&interpreted_hits), expected);
        }
        let prior = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        let after = gpu.scene_draw_stats();
        assert!(
            png.as_slice() == interpreted.render(&mut preview_gpu)?,
            "layer mask {mask}"
        );
        let (root, _, _) = aot.inspect_frame().ok_or("missing rendered layer scene")?;
        let models: Vec<_> = root
            .materialized_children()
            .filter_map(|(_, node)| {
                if let Some(Value::SceneObjects(model)) = node.properties.get("model") {
                    Some(model)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(models.len(), 2);
        assert!(std::sync::Arc::ptr_eq(models[0], models[1]));
        Ok(Frame {
            image: xrack_assets::decode_png(&png)?,
            submitted: after.color_instances - prior.color_instances,
            layers: after.layer_filtered_instances - prior.layer_filtered_instances,
            clipped: after.culled_instances - prior.culled_instances,
            shadow_passes: gpu.scene_shadow_passes(),
            uploads: [gpu.scene_mesh_uploads(), gpu.scene_instance_uploads()],
        })
    };
    let main = draw(1, -2.0, true, &["marker"])?;
    assert_eq!((main.submitted, main.layers, main.clipped), (6, 5, 1));
    assert_eq!(
        main.uploads[1], 1,
        "both views share the GPU instance buffer"
    );
    let pixel = |image: &xrack_assets::Raster, x: usize, y: usize| -> [u8; 4] {
        let i = (y * 552 + x) * 4;
        std::array::from_fn(|channel| image.rgba[i + channel])
    };
    let left_reflection = pixel(&main.image, 218, 142);
    let right_reflection = pixel(&main.image, 490, 142);
    assert!(
        left_reflection[0] > left_reflection[2].saturating_add(30),
        "{left_reflection:?}"
    );
    assert!(
        right_reflection[2] > right_reflection[0].saturating_add(30),
        "{right_reflection:?}"
    );

    let objective = draw(2, -2.0, true, &["objective"])?;
    assert_eq!(
        (objective.submitted, objective.layers, objective.clipped),
        (6, 6, 0)
    );
    assert_eq!(objective.uploads, main.uploads);
    assert_eq!(pixel(&objective.image, 218, 142), right_reflection);
    let combined = draw(-1, -2.0, true, &["objective", "marker"])?;
    assert_eq!(
        (combined.submitted, combined.layers, combined.clipped),
        (8, 3, 1)
    );
    assert_eq!(combined.uploads, main.uploads);
    let empty = draw(0, -2.0, true, &[])?;
    assert_eq!((empty.submitted, empty.layers, empty.clipped), (3, 9, 0));
    assert_eq!(empty.uploads, main.uploads);

    let base = draw(1, -2.0, true, &["marker"])?;
    let moved = draw(1, -1.4, true, &["marker"])?;
    assert!(
        moved.image.rgba == base.image.rgba,
        "excluded objects do not cast into either view"
    );
    assert_eq!(
        moved.shadow_passes, base.shadow_passes,
        "moving an excluded object reuses the shadow atlases"
    );
    let uncull = draw(1, -1.4, false, &["marker"])?;
    assert_eq!((uncull.submitted, uncull.layers, uncull.clipped), (7, 5, 0));
    assert!(uncull.image.rgba == moved.image.rgba);
    Ok(())
}

#[test]
fn scene_texture_resolution_is_independent_of_projection_and_device_scale() -> TestResult {
    use texture_size_ui::TextureSizeDemo;
    use xrack_runtime::{Length, Point2D, scene3d::ProjectedPoint3D};

    const NAME: &str = "examples/puzzle3d/texture_size.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("TextureSizeDemo")?;
    let app = TextureSizeDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    #[derive(Clone, Copy, Debug)]
    struct Settings {
        extent: [i64; 2],
        width: f64,
        quality: f64,
        dpi: f32,
        ssaa: f64,
        msaa: bool,
        square: bool,
        effects: bool,
        in_bar: bool,
    }
    struct Frame {
        image: xrack_assets::Raster,
        marker: ProjectedPoint3D,
        extent: [u32; 2],
        raster: [u32; 2],
        pixels: u64,
        passes: [u64; 6],
        uploads: [u64; 2],
    }
    let mut render = |options: Settings| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing texture-size demo")?;
        let cursor = Point2D::new(
            Length(if options.in_bar {
                20.0
            } else {
                options.width * 0.5
            }),
            Length(120.0),
        );
        app.set_pixels_wide(options.extent[0]);
        app.set_pixels_high(options.extent[1]);
        app.set_view_width(options.width);
        app.set_quality(options.quality);
        app.set_ssaa(options.ssaa);
        app.set_msaa(options.msaa);
        app.set_square_camera(options.square);
        app.set_effects(options.effects);
        app.set_cursor(cursor);
        for (name, value) in [
            ("pixels-wide", Value::Int(options.extent[0])),
            ("pixels-high", Value::Int(options.extent[1])),
            ("view-width", Value::Float(options.width)),
            ("quality", Value::Float(options.quality)),
            ("ssaa", Value::Float(options.ssaa)),
            ("msaa", Value::Bool(options.msaa)),
            ("square-camera", Value::Bool(options.square)),
            ("effects", Value::Bool(options.effects)),
            ("cursor", Value::Point2D(cursor)),
        ] {
            dynamic.set(name, value)?;
        }
        let marker = app.get_marker();
        assert!(marker.inside, "{options:?}");
        assert_eq!(
            marker.point,
            Point2D::new(Length(options.width * 0.5), Length(120.0))
        );
        assert_eq!(dynamic.get("marker")?, Value::ProjectedPoint3D(marker));
        assert_eq!(app.get_picked(), if options.in_bar { "" } else { "token" });
        assert_eq!(dynamic.get("picked")?, Value::String(app.get_picked()));
        let before = renderer.scene_draw_stats();
        let png = aot.render_scaled(&mut renderer, options.dpi)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(
            png.as_slice() == interpreted.render_scaled(&mut renderer, options.dpi)?,
            "{options:?}"
        );
        let (view, layout) = inspect(&aot)?;
        assert_eq!(layout.bounds.width, options.width as f32);
        assert_eq!(layout.bounds.height, 240.0);
        assert_eq!(view.boolean("multisampling", false), options.msaa);
        assert_eq!(
            renderer.scene_draw_stats().last_texture_size,
            after.last_texture_size
        );
        Ok(Frame {
            image: xrack_assets::decode_png(&png)?,
            marker,
            extent: after.last_texture_size,
            raster: after.last_raster_size,
            pixels: after.raster_pixels - before.raster_pixels,
            passes: [
                after.multisample_resolves - before.multisample_resolves,
                after.supersample_resolves - before.supersample_resolves,
                after.hdr_resolves - before.hdr_resolves,
                after.bloom_passes - before.bloom_passes,
                after.depth_of_field_passes - before.depth_of_field_passes,
                after.occlusion_evaluations - before.occlusion_evaluations,
            ],
            uploads: [
                renderer.scene_mesh_uploads(),
                renderer.scene_instance_uploads(),
            ],
        })
    };
    let base = Settings {
        extent: [0; 2],
        width: 480.0,
        quality: 1.0,
        dpi: 1.0,
        ssaa: 1.0,
        msaa: false,
        square: false,
        effects: false,
        in_bar: false,
    };
    let auto = render(base)?;
    assert_eq!(auto.image.size, [480, 302]);
    assert_eq!(auto.extent, [480, 240]);
    assert_eq!(auto.raster, auto.extent);
    assert_eq!(auto.pixels, 115_200);
    let half = render(Settings {
        quality: 0.5,
        ..base
    })?;
    assert_eq!(half.extent, [240, 120]);
    assert_eq!(half.pixels * 4, auto.pixels);
    assert_eq!(half.passes, [0; 6]);
    assert_eq!(half.marker, auto.marker);
    assert!(half.image.rgba != auto.image.rgba);

    let fixed = Settings {
        extent: [192, 96],
        ..base
    };
    let native = render(fixed)?;
    let retina = render(Settings { dpi: 2.0, ..fixed })?;
    assert_eq!(retina.image.size, [960, 604]);
    assert_eq!(
        (retina.extent, retina.raster, retina.pixels),
        ([192, 96], [192, 96], 18_432)
    );
    assert_eq!(native.extent, retina.extent);
    assert_eq!(native.marker, retina.marker);
    assert_eq!(native.passes, [0; 6]);
    let resized = render(Settings {
        dpi: 2.0,
        width: 376.0,
        ..fixed
    })?;
    assert_eq!(resized.extent, retina.extent);
    assert_eq!(resized.marker.point.x, Length(188.0));
    let mixed = render(Settings {
        extent: [192, 0],
        dpi: 2.0,
        quality: 0.5,
        ..base
    })?;
    assert_eq!(mixed.extent, [192, 240]);
    let mixed_height = render(Settings {
        extent: [0, 96],
        quality: 0.5,
        ..base
    })?;
    assert_eq!(mixed_height.extent, [240, 96]);
    assert_eq!(mixed.marker, auto.marker);

    let msaa = render(Settings {
        msaa: true,
        ..fixed
    })?;
    assert_eq!(msaa.passes, [1, 0, 0, 0, 0, 0]);
    assert_eq!(msaa.extent, native.extent);
    let ssaa = render(Settings {
        ssaa: 2.0,
        msaa: true,
        ..fixed
    })?;
    assert_eq!((ssaa.extent, ssaa.raster), ([192, 96], [384, 192]));
    assert_eq!(ssaa.passes, [0, 1, 0, 0, 0, 0]);

    let effects = render(Settings {
        extent: [192, 144],
        effects: true,
        ..base
    })?;
    assert_eq!(effects.extent, [192, 144]);
    assert_eq!(effects.passes[..3], [0, 0, 1]);
    assert!(effects.passes[3] > 0);
    assert_eq!(effects.passes[4..], [3, 1]);
    let square = render(Settings {
        square: true,
        in_bar: true,
        ..base
    })?;
    assert_eq!(square.extent, [240, 240]);
    let square_fixed = render(Settings {
        square: true,
        in_bar: true,
        ..fixed
    })?;
    assert_eq!(square_fixed.extent, [192, 96]);
    assert_eq!(square_fixed.marker, square.marker);
    let bar_pixel = 4 * (120 * 480 + 20);
    assert_eq!(
        square_fixed.image.rgba[bar_pixel..bar_pixel + 4],
        [0x26, 0x34, 0x49, 0xff]
    );
    let limited = render(Settings {
        extent: [i64::MAX, i64::MAX],
        ..base
    })?;
    assert!(limited.pixels <= auto.pixels + 4_194_304);
    assert_eq!(limited.extent[0], limited.extent[1]);
    assert_eq!(limited.uploads, auto.uploads);
    Ok(())
}

#[test]
fn local_reflections_follow_probe_boxes_and_keep_geometry_resident() -> TestResult {
    use reflection_probes_ui::ReflectionProbesDemo;

    const NAME: &str = "examples/puzzle3d/reflection_probes.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ReflectionProbesDemo")?;
    let app = ReflectionProbesDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    #[derive(Clone, Copy, Debug)]
    struct Settings {
        enabled: bool,
        parallax: bool,
        receive_right: bool,
        culling: bool,
        warm_x: f64,
        orbit: f64,
    }
    struct Frame {
        image: xrack_assets::Raster,
        uploads: [u64; 3],
        draws: u64,
        instances: u64,
    }
    let mut render = |settings: Settings| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing reflection demo")?;
        app.set_enabled(settings.enabled);
        app.set_parallax(settings.parallax);
        app.set_receive_right(settings.receive_right);
        app.set_culling(settings.culling);
        app.set_warm_x(settings.warm_x);
        app.set_orbit(settings.orbit);
        for (name, value) in [
            ("enabled", Value::Bool(settings.enabled)),
            ("parallax", Value::Bool(settings.parallax)),
            ("receive-right", Value::Bool(settings.receive_right)),
            ("culling", Value::Bool(settings.culling)),
            ("warm-x", Value::Float(settings.warm_x)),
            ("orbit", Value::Float(settings.orbit)),
        ] {
            dynamic.set(name, value)?;
        }
        let prior = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let stats = renderer.scene_draw_stats();
        assert!(
            png.as_slice() == interpreted.render(&mut renderer)?,
            "{settings:?}"
        );
        Ok(Frame {
            image: xrack_assets::decode_png(&png)?,
            uploads: [
                renderer.scene_mesh_uploads(),
                renderer.scene_instance_uploads(),
                renderer.scene_environment_uploads(),
            ],
            draws: stats.color_draws - prior.color_draws,
            instances: stats.color_instances - prior.color_instances,
        })
    };
    let local = Settings {
        enabled: true,
        parallax: true,
        receive_right: false,
        culling: true,
        warm_x: 1.7,
        orbit: 0.0,
    };
    let global = render(Settings {
        enabled: false,
        ..local
    })?;
    assert_eq!((global.draws, global.instances), (1, 4));
    let scoped = render(local)?;
    assert_eq!((scoped.draws, scoped.instances), (3, 4));
    assert_eq!(scoped.uploads[..2], global.uploads[..2]);
    assert!(scoped.uploads[2] > global.uploads[2]);
    let cool = [128, 125];
    let overlap = [232, 125];
    let warm = [348, 125];
    let right = [452, 125];
    let pixel = |frame: &Frame, [x, y]: [usize; 2]| -> [u8; 4] {
        let offset = 4 * (y * frame.image.size[0] as usize + x);
        std::array::from_fn(|i| frame.image.rgba[offset + i])
    };
    let blue = |frame: &Frame, point| {
        let [r, _, b, _] = pixel(frame, point);
        assert!(
            b > r.saturating_add(25),
            "expected blue at {point:?}, read R={r} B={b}"
        );
    };
    let amber = |frame: &Frame, point| {
        let [r, _, b, _] = pixel(frame, point);
        assert!(
            r > b.saturating_add(25),
            "expected amber at {point:?}, read R={r} B={b}"
        );
    };
    blue(&scoped, cool);
    blue(&scoped, overlap);
    amber(&scoped, warm);
    assert_eq!(pixel(&scoped, right), pixel(&global, right));
    assert_ne!(pixel(&scoped, cool), pixel(&global, cool));
    let unculled = render(Settings {
        culling: false,
        ..local
    })?;
    assert_eq!(unculled.instances, 5);
    assert!(unculled.image.rgba == scoped.image.rgba);
    assert_eq!(unculled.uploads, scoped.uploads);
    let nearer = render(Settings {
        warm_x: -0.6,
        ..local
    })?;
    amber(&nearer, overlap);
    assert_eq!(nearer.uploads, scoped.uploads);
    let tie = render(Settings {
        warm_x: -3.8,
        ..local
    })?;
    blue(&tie, overlap);
    let receiver = render(Settings {
        receive_right: true,
        ..local
    })?;
    amber(&receiver, right);
    assert_eq!(receiver.draws, 2);
    assert_eq!(receiver.uploads, scoped.uploads);

    let without_parallax = render(Settings {
        parallax: false,
        ..local
    })?;
    let view_bytes = 4 * scoped.image.size[0] as usize * 250;
    assert!(scoped.image.rgba[..view_bytes] != without_parallax.image.rgba[..view_bytes]);
    assert_eq!(without_parallax.uploads, scoped.uploads);
    let orbit = render(Settings {
        orbit: 11.0,
        ..local
    })?;
    assert_eq!(orbit.uploads, scoped.uploads);
    Ok(())
}

#[test]
fn animation_crossfade_shares_one_pose_between_pixels_skin_morph_and_rays() -> TestResult {
    use animation_blend_ui::AnimationBlendDemo;
    use xrack_runtime::scene3d::{Point3D, ProjectedPoint3D, SceneObject};

    const NAME: &str = "examples/puzzle3d/animation_blend.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("AnimationBlendDemo")?;
    let app = AnimationBlendDemo::new();
    assert_eq!(
        dynamic.get("dolly")?,
        Value::ProjectedPoint3D(app.get_dolly())
    );
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let cursor = SceneCamera::new(0.0, 0.0, 9.0, [580.0, 244.0])
        .ok_or("missing orbit camera")?
        .project(Point3D::new(-4.5, 0.0, 0.0))
        .point;
    #[derive(Clone, Copy, Debug)]
    struct Request {
        blend: f64,
        time: [f64; 2],
        looping: [bool; 2],
        target: &'static str,
    }
    struct Frame {
        scene_pixels: Vec<u8>,
        objects: Vec<SceneObject>,
        picked: String,
        dolly: ProjectedPoint3D,
        uploads: [u64; 4],
    }
    let mut frame = |request: Request| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing animation demo")?;
        app.set_blend(request.blend);
        app.set_from_time(request.time[0]);
        app.set_to_time(request.time[1]);
        app.set_from_loop(request.looping[0]);
        app.set_to_loop(request.looping[1]);
        app.set_motion_to(request.target.into());
        app.set_cursor(cursor);
        for (name, value) in [
            ("blend", Value::Float(request.blend)),
            ("from-time", Value::Float(request.time[0])),
            ("to-time", Value::Float(request.time[1])),
            ("from-loop", Value::Bool(request.looping[0])),
            ("to-loop", Value::Bool(request.looping[1])),
            ("motion-to", Value::String(request.target.into())),
            ("cursor", Value::Point2D(cursor)),
        ] {
            dynamic.set(name, value)?;
        }
        let picked = app.get_picked();
        let dolly = app.get_dolly();
        assert_eq!(dynamic.get("picked")?, Value::String(picked.clone()));
        assert_eq!(dynamic.get("dolly")?, Value::ProjectedPoint3D(dolly));
        let png = aot.render(&mut renderer)?.to_vec();
        assert!(
            png.as_slice() == interpreted.render(&mut renderer)?,
            "{request:?}"
        );
        let (view, _) = inspect(&interpreted)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing crossfade model".into());
        };
        // The controls also show the inactive playback time, outside the viewport.
        let image = xrack_assets::decode_png(&png)?;
        let viewport_bytes = 4 * image.size[0] as usize * 244;
        Ok(Frame {
            scene_pixels: image.rgba[..viewport_bytes].to_vec(),
            objects: scene.to_vec(),
            picked,
            dolly,
            uploads: [
                renderer.scene_mesh_uploads(),
                renderer.scene_skin_uploads(),
                renderer.scene_morph_uploads(),
                renderer.scene_shadow_passes(),
            ],
        })
    };
    let defaults = Request {
        blend: 0.5,
        time: [1.0; 2],
        looping: [false; 2],
        target: "Turn",
    };
    let from = frame(Request {
        blend: 0.0,
        ..defaults
    })?;
    let inactive_target = frame(Request {
        blend: -0.2,
        time: [1.0, 0.4],
        target: "Curve",
        ..defaults
    })?;
    assert!(inactive_target.scene_pixels == from.scene_pixels);
    assert_eq!(inactive_target.uploads, from.uploads);
    let to = frame(Request {
        blend: 1.0,
        ..defaults
    })?;
    let inactive_source = frame(Request {
        blend: 2.0,
        time: [0.25, 1.0],
        ..defaults
    })?;
    assert!(inactive_source.scene_pixels == to.scene_pixels);
    assert_eq!(inactive_source.uploads, to.uploads);
    let blended = frame(defaults)?;
    assert!(blended.scene_pixels != from.scene_pixels);
    assert!(blended.scene_pixels != to.scene_pixels);
    assert_eq!(blended.picked, "movement");
    assert_eq!(from.picked, "backdrop");
    assert_eq!(to.picked, "backdrop");
    assert_eq!(blended.uploads[0], from.uploads[0]);
    assert!(
        blended.uploads[1..]
            .iter()
            .zip(to.uploads[1..].iter())
            .all(|(new, old)| new > old)
    );

    let near = |a: f32, b: f32| assert!((a - b).abs() < 1e-4, "{a} != {b}");
    let rows = blended.objects[0].transform_rows();
    near(rows[0][3], -4.5);
    near(rows[0][0], 0.5_f32.sqrt());
    near(rows[1][0], 0.5_f32.sqrt());
    let palette = |frame: &Frame| -> TestResult<_> {
        Ok(frame
            .objects
            .iter()
            .find(|object| object.id == "skin")
            .and_then(SceneObject::skin_palette)
            .ok_or("missing blended skin")?
            .clone())
    };
    let [a, b, mixed] = [palette(&from)?, palette(&to)?, palette(&blended)?];
    assert_ne!(a, b);
    for ((&mixed, &a), &b) in mixed
        .iter()
        .flatten()
        .flatten()
        .zip(a.iter().flatten().flatten())
        .zip(b.iter().flatten().flatten())
    {
        near(mixed, (a + b) * 0.5);
    }
    let morph = |frame: &Frame| -> Vec<Vec<f32>> {
        frame
            .objects
            .iter()
            .filter(|object| object.id == "morph")
            .map(|object| {
                object
                    .morph_weights()
                    .map_or_else(|| vec![0.0; 2], |weights| weights.to_vec())
            })
            .collect()
    };
    let [a, b, mixed] = [morph(&from), morph(&to), morph(&blended)];
    assert!(mixed.len() == 2 && a != b);
    for ((a, b), mixed) in a
        .into_iter()
        .flatten()
        .zip(b.into_iter().flatten())
        .zip(mixed.into_iter().flatten())
    {
        near(mixed, (a + b) * 0.5);
    }
    assert!(blended.dolly.valid && to.dolly.point != from.dolly.point);
    near(
        blended.dolly.point.x.0 as f32,
        ((from.dolly.point.x.0 + to.dolly.point.x.0) * 0.5) as f32,
    );

    let step = frame(Request {
        time: [0.25, 1.0],
        target: "#1",
        ..defaults
    })?;
    near(step.objects[0].transform_rows()[0][3], -4.25);
    let scale = frame(Request {
        target: "Curve",
        ..defaults
    })?;
    near(scale.objects[0].transform_rows()[0][0], 1.5);
    let clamped = frame(Request {
        time: [0.25, 0.5],
        ..defaults
    })?;
    let looped = frame(Request {
        time: [2.25, 2.5],
        looping: [true; 2],
        ..defaults
    })?;
    assert!(looped.scene_pixels == clamped.scene_pixels);
    assert_eq!(looped.dolly, clamped.dolly);
    let rest = frame(Request {
        blend: 1.0,
        target: "missing",
        ..defaults
    })?;
    near(rest.objects[0].transform_rows()[0][3], -5.5);
    near(rest.objects[0].transform_rows()[0][0], 1.0);
    Ok(())
}

#[test]
fn camera_target_up_and_picking_share_one_pose_in_both_backends() -> TestResult {
    use camera_aim_ui::CameraAimDemo;
    use xrack_runtime::{Point3D, Quaternion3D, Vector3D};

    const NAME: &str = "examples/puzzle3d/camera_aim.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let dynamic =
        xrack_interpreter::Program::from_module(&preview.module)?.create("CameraAimDemo")?;
    let app = CameraAimDemo::new();
    let weak = app.as_weak();
    let focus = app.get_focus();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    let standard_eye = Point3D::new(0.0, 2.5, 8.0);
    let moving_eye = Point3D::new(0.9, 3.6, 8.3);
    let overhead = Point3D::new(focus.x, 8.0, focus.z);
    let target = Point3D::new(1.6, 0.5, -0.6);
    let world_up = Vector3D::new(0.0, 1.0, 0.0);
    let map_up = Vector3D::new(0.0, 0.0, -1.0);
    let view = [440.0, 274.0];
    let near = |a: f64, b: f64| assert!((a - b).abs() < 2e-3, "{a} != {b}");
    let camera = |preview: &PreviewScene| -> TestResult<SceneCamera> {
        let (node, _) = inspect(preview)?;
        let Some(Value::SceneObjects(scene)) = node.properties.get("model") else {
            return Err("missing prepared camera scene".into());
        };
        scene
            .camera("eye", view)
            .ok_or("missing aimed camera".into())
    };
    let mut pictures = Vec::new();
    let mut uploads = None;
    for (eye, up, follow, visible, expected) in [
        (standard_eye, world_up, false, true, "point"),
        (moving_eye, world_up, true, true, "target"),
        (
            moving_eye,
            Vector3D::new(1.0, 0.0, 0.0),
            true,
            true,
            "target",
        ),
        (overhead, map_up, true, false, "point"),
        (overhead, world_up, true, false, "point"),
    ] {
        let mut app = weak.upgrade().ok_or("missing camera aim demo")?;
        app.set_eye(eye);
        app.set_target(target);
        app.set_up(up);
        app.set_follow_node(follow);
        app.set_show_target(visible);
        for (name, value) in [
            ("eye", eye.into()),
            ("target", target.into()),
            ("up", up.into()),
            ("follow-node", follow.into()),
            ("show-target", visible.into()),
        ] {
            dynamic.set(name, value)?;
        }
        let png = aot.render(&mut renderer)?.to_vec();
        assert_eq!(png.as_slice(), interpreted.render(&mut renderer)?);
        let current_uploads = renderer.scene_mesh_uploads();
        assert_eq!(*uploads.get_or_insert(current_uploads), current_uploads);
        pictures.push(png);

        let projected = app.get_projected();
        assert!(projected.inside);
        near(projected.point.x.0, view[0] / 2.0);
        near(projected.point.y.0, view[1] / 2.0);
        assert_eq!(dynamic.get("projected")?, projected.into());
        assert_eq!(app.get_aimed_hit().object_id, expected);
        assert_eq!(dynamic.get("aimed-hit")?, app.get_aimed_hit().into());
        assert_eq!(dynamic.get("marker-aim")?, app.get_marker_aim().into());
        assert_eq!(dynamic.get("view-aim")?, app.get_view_aim().into());
        let pose = camera(&aot)?;
        assert!(pose.same_frustum(&camera(&interpreted)?));
        if eye == overhead {
            let north = pose.project(Point3D::new(focus.x, focus.y, focus.z - 1.0));
            near(north.point.x.0, view[0] / 2.0);
            assert!(north.point.y.0 < view[1] / 2.0);
        }
    }
    assert_ne!(pictures[0], pictures[1]);
    assert_ne!(pictures[1], pictures[2]);
    assert_ne!(pictures[2], pictures[3]);

    let mut app = weak.upgrade().ok_or("missing camera aim demo")?;
    let roll = Quaternion3D::from_axis_angle(Vector3D::new(0.0, 0.0, 1.0), 90.0);
    app.set_eye(focus);
    app.set_authored_orientation(roll);
    dynamic.set("eye", focus.into())?;
    dynamic.set("authored-orientation", roll.into())?;
    let png = aot.render(&mut renderer)?.to_vec();
    assert_eq!(png.as_slice(), interpreted.render(&mut renderer)?);
    let pose = camera(&aot)?;
    let ahead = pose.project(Point3D::new(focus.x, focus.y, focus.z - 3.0));
    near(ahead.point.x.0, view[0] / 2.0);
    near(ahead.point.y.0, view[1] / 2.0);
    let up = pose.project(Point3D::new(focus.x - 1.0, focus.y, focus.z - 3.0));
    near(up.point.x.0, view[0] / 2.0);
    assert!(up.point.y.0 < view[1] / 2.0);
    Ok(())
}

#[test]
fn reverse_device_depth_renders_the_near_panel_at_a_forward_depth_tie() -> TestResult {
    use reverse_depth_ui::ReverseDepthDemo;

    const NAME: &str = "examples/puzzle3d/reverse_depth.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let dynamic =
        xrack_interpreter::Program::from_module(&preview.module)?.create("ReverseDepthDemo")?;
    let app = ReverseDepthDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut uploads = None;
    for (reversed, msaa) in [(false, false), (true, false), (true, true), (false, true)] {
        let mut app = weak.upgrade().ok_or("missing depth demo")?;
        app.set_reversed(reversed);
        app.set_msaa(msaa);
        dynamic.set("reversed", reversed.into())?;
        dynamic.set("msaa", msaa.into())?;
        assert_eq!(app.get_picked(), "near-panel");
        assert_eq!(dynamic.get("picked")?, Value::String("near-panel".into()));

        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png.as_slice(), interpreted.render(&mut gpu)?);
        let current = gpu.scene_mesh_uploads();
        assert_eq!(*uploads.get_or_insert(current), current);
        let raster = xrack_assets::decode_png(&png)?;
        let pixel = 4 * (126 * 432 + 216);
        if reversed {
            assert!(
                raster.rgba[pixel] > raster.rgba[pixel + 2].saturating_add(30),
                "expected the orange near panel, msaa={msaa}",
            );
        }
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing precision scene".into());
        };
        let camera = scene
            .camera("eye", [432.0, 252.0])
            .ok_or("missing precision camera")?;
        assert_eq!(camera.reverse_depth(), reversed);
        let [far, near] = [0.0, 0.02].map(|z| {
            let clip = camera.clip_position([0.0, 0.0, z]);
            clip[2] / clip[3]
        });
        if reversed {
            assert!(near > far);
        } else {
            assert_eq!(near, far);
        }
    }
    Ok(())
}

#[test]
fn custom_matrices_match_projection_picking_and_gpu_effects_in_both_backends() -> TestResult {
    use custom_camera_ui::CustomCameraDemo;

    const NAME: &str = "examples/puzzle3d/custom_camera.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let dynamic =
        xrack_interpreter::Program::from_module(&preview.module)?.create("CustomCameraDemo")?;
    let app = CustomCameraDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let close = |a: f64, b: f64| assert!((a - b).abs() < 1e-3, "{a} != {b}");

    let mut draw =
        |parallel: bool, msaa, culling, effects, opengl, reversed, skew| -> TestResult<_> {
            let mut app = weak.upgrade().ok_or("missing matrix camera demo")?;
            app.set_parallel(parallel);
            app.set_msaa(msaa);
            app.set_geometry_culling(culling);
            app.set_effects(effects);
            app.set_opengl(opengl);
            app.set_reversed(reversed);
            app.set_skew(skew);
            for (name, value) in [
                ("parallel", parallel.into()),
                ("msaa", msaa.into()),
                ("geometry-culling", culling.into()),
                ("effects", effects.into()),
                ("opengl", opengl.into()),
                ("reversed", reversed.into()),
                ("skew", Value::Float(skew)),
            ] {
                dynamic.set(name, value)?;
            }
            let projected = app.get_projected();
            let restored = app.get_restored();
            assert!(projected.valid && projected.inside);
            let [x, y, z, w] = if parallel {
                [
                    0.48 * 0.9 + 0.45 * skew * 0.5 - 0.18 * 3.0 + 0.15,
                    0.6 * 0.5 + 0.13 * 3.0 - 0.25,
                    0.5,
                    1.0,
                ]
            } else {
                [
                    1.44 * 0.9 + skew * 0.5 - 0.06 * 3.0 + 0.12,
                    0.12 * 0.9 + 1.6 * 0.5 + 0.02 * 3.0 + 0.08,
                    0.01 * 0.9 - 0.02 * 0.5 + 1.25 * 3.0 - 1.25,
                    0.045 * 0.9 - 0.035 * 0.5 + 3.0,
                ]
            };
            let sign = if app.get_mirrored() { -1.0 } else { 1.0 };
            close(projected.point.x.0, (sign * x / w * 0.5 + 0.5) * 432.0);
            close(projected.point.y.0, (0.5 - y / w * 0.5) * 272.0);
            close(projected.depth, 3.0);
            close(restored.x, 0.9);
            close(restored.y, 0.5);
            close(restored.z, 5.0);
            assert_eq!(app.get_picked(), "cyan");
            assert_eq!(dynamic.get("projected")?, projected.into());
            assert_eq!(dynamic.get("restored")?, restored.into());
            assert_eq!(dynamic.get("picked")?, Value::String("cyan".into()));
            let panel = app.get_panel_projection();
            assert!(panel.inside);
            assert_eq!(app.get_panel_picked(), "peach");
            assert_eq!(dynamic.get("panel-picked")?, Value::String("peach".into()));

            let before = gpu.scene_draw_stats();
            let png = aot.render(&mut gpu)?.to_vec();
            let after = gpu.scene_draw_stats();
            assert_eq!(png.as_slice(), interpreted.render(&mut gpu)?);
            let (node, _) = inspect(&aot)?;
            let Some(Value::SceneObjects(scene)) = node.properties.get("model") else {
                return Err("missing custom camera scene".into());
            };
            let camera = scene
                .camera("eye", [432.0, 272.0])
                .ok_or("missing matrix lens")?;
            assert!(camera.has_custom_projection());
            assert_eq!(camera.is_orthographic(), parallel);
            assert_eq!(camera.reverses_winding(), app.get_mirrored());
            let clip = camera.clip_position([0.9, 0.5, 5.0]);
            let expected_depth = if app.get_device_reversed() {
                1.0 - z / w
            } else {
                z / w
            };
            assert_eq!(camera.reverse_depth(), app.get_device_reversed());
            close(f64::from(clip[2] / clip[3]), expected_depth);
            for (name, before, after) in [
                (
                    "occlusion",
                    before.occlusion_evaluations,
                    after.occlusion_evaluations,
                ),
                (
                    "reflection",
                    before.screen_reflection_resolves,
                    after.screen_reflection_resolves,
                ),
                (
                    "indirect light",
                    before.screen_indirect_resolves,
                    after.screen_indirect_resolves,
                ),
                (
                    "depth of field",
                    before.depth_of_field_passes,
                    after.depth_of_field_passes,
                ),
            ] {
                assert_eq!(
                    after > before,
                    effects,
                    "{name}, parallel={parallel}, msaa={msaa}"
                );
            }
            let pixels = xrack_assets::decode_png(&png)?.rgba;
            if !effects {
                let x = panel.point.x.0.floor() as usize;
                let y = panel.point.y.0.floor() as usize;
                let start = (y * 432 + x) * 4;
                let [r, _, b, _] = <[u8; 4]>::try_from(&pixels[start..start + 4])?;
                assert!(
                    r > b.saturating_add(8),
                    "single-sided reflected panel pixel {r},{b}, camera mirror {}",
                    app.get_mirrored(),
                );
            }
            Ok((
                pixels[..432 * 272 * 4].to_vec(),
                after.color_instances - before.color_instances,
                gpu.scene_mesh_uploads(),
            ))
        };
    for (parallel, msaa) in [(false, false), (true, true)] {
        let mut app = weak.upgrade().ok_or("missing matrix camera demo")?;
        app.set_mirrored(false);
        dynamic.set("mirrored", false.into())?;
        app.set_device_reversed(false);
        dynamic.set("device-reversed", false.into())?;
        let (all, count, uploads) = draw(parallel, msaa, false, false, false, false, 0.35)?;
        let (culled, kept, _) = draw(parallel, msaa, true, false, false, false, 0.35)?;
        assert_eq!(culled, all);
        assert!(kept < count);
        app.set_mirrored(true);
        dynamic.set("mirrored", true.into())?;
        let (mirrored, _, _) = draw(parallel, msaa, true, false, false, false, 0.35)?;
        assert_ne!(mirrored, culled);
        app.set_device_reversed(true);
        dynamic.set("device-reversed", true.into())?;
        let (with_effects, _, after) = draw(parallel, msaa, true, true, true, true, 0.35)?;
        assert_ne!(with_effects, mirrored);
        assert_eq!(uploads, after);
    }
    Ok(())
}

#[test]
fn oblique_depth_agrees_for_picking_culling_and_screen_effects() -> TestResult {
    use oblique_camera_ui::ObliqueCameraDemo;

    const NAME: &str = "examples/puzzle3d/oblique_camera.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let dynamic =
        xrack_interpreter::Program::from_module(&preview.module)?.create("ObliqueCameraDemo")?;
    let app = ObliqueCameraDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    let mut frame = |parallel: bool, msaa, tilted, culling, effects| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing oblique camera demo")?;
        app.set_parallel(parallel);
        app.set_msaa(msaa);
        app.set_tilted(tilted);
        app.set_culling(culling);
        app.set_effects(effects);
        for (name, value) in [
            ("parallel", parallel),
            ("msaa", msaa),
            ("tilted", tilted),
            ("culling", culling),
            ("effects", effects),
        ] {
            dynamic.set(name, value.into())?;
        }

        let before = renderer.scene_draw_stats();
        let image = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert_eq!(image.as_slice(), interpreted.render(&mut renderer)?);
        for (name, before, after) in [
            (
                "occlusion",
                before.occlusion_evaluations,
                after.occlusion_evaluations,
            ),
            (
                "reflection",
                before.screen_reflection_resolves,
                after.screen_reflection_resolves,
            ),
            (
                "indirect lighting",
                before.screen_indirect_resolves,
                after.screen_indirect_resolves,
            ),
            (
                "depth of field",
                before.depth_of_field_passes,
                after.depth_of_field_passes,
            ),
        ] {
            assert_eq!(
                after > before,
                effects,
                "{name}, parallel={parallel}, msaa={msaa}"
            );
        }
        for (name, hit, expected) in [
            (
                "picked-near-left",
                app.get_picked_near_left(),
                if tilted { "" } else { "near-left" },
            ),
            (
                "picked-near-right",
                app.get_picked_near_right(),
                "near-right",
            ),
            ("picked-far-left", app.get_picked_far_left(), "far-left"),
            (
                "picked-far-right",
                app.get_picked_far_right(),
                if tilted { "" } else { "far-right" },
            ),
        ] {
            assert_eq!(hit, expected);
            assert_eq!(dynamic.get(name)?, hit.into());
        }
        let raster = xrack_assets::decode_png(&image)?;
        Ok((
            raster.rgba[..432 * 252 * 4].to_vec(),
            renderer.scene_mesh_uploads(),
            after.color_instances - before.color_instances,
        ))
    };
    for (parallel, msaa) in [(true, false), (false, true)] {
        let mut app = weak.upgrade().ok_or("missing oblique camera demo")?;
        app.set_device_reversed(false);
        dynamic.set("device-reversed", false.into())?;
        let (flat, uploads, _) = frame(parallel, msaa, false, false, false)?;
        let (unculled, _, full_draws) = frame(parallel, msaa, true, false, false)?;
        let (clipped, same_uploads, culled_draws) = frame(parallel, msaa, true, true, false)?;
        assert_eq!(unculled, clipped);
        assert!(culled_draws < full_draws);
        assert_ne!(clipped, flat);
        assert_eq!(same_uploads, uploads);
        let (shaded, _, _) = frame(parallel, msaa, true, true, true)?;
        assert_ne!(shaded, clipped);
        app.set_device_reversed(true);
        dynamic.set("device-reversed", true.into())?;
        let (reversed, _, _) = frame(parallel, msaa, true, true, true)?;
        assert_ne!(reversed, clipped);
    }
    Ok(())
}

#[test]
fn orthographic_width_updates_horizontal_span_in_both_backends() -> TestResult {
    use scene3d_model_ui::OrthoExtentProbe;

    const NAME: &str = "tests/scene3d_model.xr";
    let preview = xrack_compiler::prepare_preview(
        include_str!("scene3d_model.xr"),
        NAME,
        Default::default(),
    )?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("OrthoExtentProbe")?;
    let mut app = OrthoExtentProbe::new();
    for (span, screen_x, valid) in [(8.0, 150.0, true), (5.0, 180.0, true), (0.0, 0.0, false)] {
        app.set_span(span);
        dynamic.set("span", span.into())?;
        let projection = app.get_projected();
        assert_eq!(projection.valid, valid);
        assert_eq!(dynamic.get("projected")?, projection.into());
        if valid {
            assert!((projection.point.x.0 - screen_x).abs() < 1e-4);
            assert_eq!(projection.point.y.0, 40.0);
        }
    }
    Ok(())
}

#[test]
fn off_axis_clip_bounds_agree_in_raster_culling_picking_and_shadows() -> TestResult {
    use off_axis_camera_ui::OffAxisCameraDemo;
    use xrack_runtime::{Length, Point2D, scene3d::Point3D};

    const NAME: &str = "examples/puzzle3d/off_axis_camera.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("OffAxisCameraDemo")?;
    let app = OffAxisCameraDemo::new();
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-3, "{a} != {b}");
    let projection = app.get_projected();
    assert!(projection.inside);
    near(projection.point.x.0, 120.0);
    near(projection.point.y.0, 150.0);
    near(projection.depth, 8.0);
    let round_trip = app.get_unprojected();
    near(round_trip.x, 0.0);
    near(round_trip.y, 0.0);
    near(round_trip.z, 0.0);
    assert_eq!(
        dynamic.get("projected")?,
        Value::ProjectedPoint3D(projection)
    );
    assert_eq!(dynamic.get("unprojected")?, Value::Point3D(round_trip));
    assert_eq!(app.get_picked(), "center");

    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let cursor = Point2D::new(Length(330.0), Length(128.0));
    let mut frame = |shift: [f64; 2], half_width, culling, effects| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing frustum demo")?;
        app.set_horizontal(shift[0]);
        app.set_vertical(shift[1]);
        app.set_half_width(half_width);
        app.set_culling(culling);
        app.set_effects(effects);
        app.set_cursor(cursor);
        for (name, value) in [
            ("horizontal", Value::Float(shift[0])),
            ("vertical", Value::Float(shift[1])),
            ("half-width", Value::Float(half_width)),
            ("culling", Value::Bool(culling)),
            ("effects", Value::Bool(effects)),
            ("cursor", Value::Point2D(cursor)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let previous_shadows = renderer.scene_shadow_passes();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert_eq!(renderer.scene_shadow_passes() > previous_shadows, effects);
        assert_eq!(
            after.occlusion_evaluations - before.occlusion_evaluations,
            u64::from(effects)
        );
        assert!(
            png.as_slice() == interpreted.render(&mut renderer)?,
            "shift {shift:?}, effects {effects}"
        );
        assert_eq!(dynamic.get("picked")?, Value::String(app.get_picked()));
        assert_eq!(
            dynamic.get("projected")?,
            Value::ProjectedPoint3D(app.get_projected())
        );
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing frustum scene".into());
        };
        Ok((
            xrack_assets::decode_png(&png)?,
            scene.camera("lens", [360.0, 240.0]),
            after.color_instances - before.color_instances,
            renderer.scene_mesh_uploads(),
            renderer.scene_instance_uploads(),
            app.get_picked(),
        ))
    };

    let (centered, _, all, meshes, instances, hit) = frame([0.0; 2], 0.75, false, false)?;
    assert!(hit.is_empty());
    let (culled, _, kept, _, _, _) = frame([0.0; 2], 0.75, true, false)?;
    assert_eq!(culled.rgba, centered.rgba);
    assert_eq!((all, kept), (4, 2));

    let (shifted, lens, kept, same_meshes, same_instances, hit) =
        frame([0.25, 0.125], 0.75, true, false)?;
    assert_eq!(hit, "right");
    assert_eq!(kept, 3);
    assert_eq!((same_meshes, same_instances), (meshes, instances));
    let lens = lens.ok_or("missing shifted camera")?;
    assert_eq!(lens.eye_position(), [0.0, 0.0, 8.0]);
    assert!(lens.project(Point3D::new(7.0, 0.0, 0.0)).inside);
    assert!(!lens.project(Point3D::new(-7.0, 0.0, 0.0)).inside);
    let (corners, center, radius) = lens.frustum_slice(1.0, 4.0);
    for (index, [x, y, z]) in corners.into_iter().enumerate() {
        let p = lens.project(Point3D::new(f64::from(x), f64::from(y), f64::from(z)));
        near(p.point.x.0, if index & 1 == 0 { 0.0 } else { 360.0 });
        near(p.point.y.0, if index & 2 == 0 { 240.0 } else { 0.0 });
        near(p.depth, if index & 4 == 0 { 1.0 } else { 4.0 });
        let distance = (x - center[0]).hypot(y - center[1]).hypot(z - center[2]);
        assert!(distance <= radius + 1e-4);
    }
    let pixel = |raster: &xrack_assets::Raster, x: usize, y: usize| -> [u8; 3] {
        let index = 4 * (y * 360 + x);
        [
            raster.rgba[index],
            raster.rgba[index + 1],
            raster.rgba[index + 2],
        ]
    };
    // The same sky direction follows the geometry's (−60, +30) pixel shift.
    for (x, y) in [(210, 45), (290, 22), (317, 79)] {
        let a = pixel(&centered, x, y);
        let b = pixel(&shifted, x - 60, y + 30);
        assert!(
            a.into_iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1),
            "{a:?} != {b:?}"
        );
    }
    let (with_effects, ..) = frame([0.25, 0.125], 0.75, true, true)?;
    assert_ne!(with_effects.rgba, shifted.rgba);

    let (invalid, no_lens, draws, _, _, hit) = frame([0.25, 0.125], 0.0, true, false)?;
    assert!(no_lens.is_none());
    assert!(hit.is_empty());
    assert_eq!(draws, 0);
    assert_eq!(pixel(&invalid, 180, 120), [0x17, 0x23, 0x33]);

    let mut app = weak.upgrade().ok_or("missing off-axis camera demo")?;
    app.set_parallel(true);
    dynamic.set("parallel", true.into())?;
    let (unfiltered, parallel, all, _, _, hit) = frame([0.25, 0.125], 0.75, false, false)?;
    assert_eq!(hit, "right");
    let parallel = parallel.ok_or("missing orthographic camera")?;
    assert!(parallel.is_orthographic());
    let (filtered, _, kept, _, _, _) = frame([0.25, 0.125], 0.75, true, false)?;
    assert_eq!(unfiltered.rgba, filtered.rgba);
    assert!(kept < all);
    for z in [0.0, 6.0] {
        let point = Point3D::new(0.0, 0.0, z);
        let projection = parallel.project(point);
        near(projection.point.x.0, 120.0);
        near(projection.point.y.0, 150.0);
        near(projection.depth, 8.0 - z);
        let original = parallel
            .unproject(projection.point, projection.depth)
            .ok_or("missing orthographic inverse")?;
        near(original.x, point.x);
        near(original.y, point.y);
        near(original.z, point.z);
    }
    let (corners, center, radius) = parallel.frustum_slice(1.0, 4.0);
    for (value, expected) in center.into_iter().zip([2.0, 1.0, 5.5]) {
        near(f64::from(value), expected);
    }
    for (index, [x, y, z]) in corners.into_iter().enumerate() {
        let point = parallel.project(Point3D::new(f64::from(x), f64::from(y), f64::from(z)));
        near(point.point.x.0, if index & 1 == 0 { 0.0 } else { 360.0 });
        near(point.point.y.0, if index & 2 == 0 { 240.0 } else { 0.0 });
        near(point.depth, if index & 4 == 0 { 1.0 } else { 4.0 });
        assert!((x - center[0]).hypot(y - center[1]).hypot(z - center[2]) <= radius + 1e-4);
    }
    let (with_effects, ..) = frame([0.25, 0.125], 0.75, true, true)?;
    assert_ne!(with_effects.rgba, filtered.rgba);
    app.set_half_height(0.25);
    dynamic.set("half-height", 0.25.into())?;
    let (zoomed, _, _, same_meshes, _, _) = frame([0.25, 0.125], 0.75, true, false)?;
    assert_eq!(same_meshes, meshes);
    assert_ne!(zoomed.rgba, filtered.rgba);
    let point = app.get_projected();
    near(point.point.x.0, 120.0);
    near(point.point.y.0, 180.0);
    let (invalid, no_lens, draws, _, _, hit) = frame([0.25, 0.125], 0.0, true, false)?;
    assert!(no_lens.is_none());
    assert!(hit.is_empty());
    assert_eq!(draws, 0);
    assert_eq!(pixel(&invalid, 180, 120), [0x17, 0x23, 0x33]);
    Ok(())
}

#[test]
fn gltf_lights_follow_animated_nodes_and_use_the_same_buffers_in_both_backends() -> TestResult {
    use xrack_runtime::scene3d::{SceneLight, SceneLightSource};

    const NAME: &str = "examples/puzzle3d/punctual_lights.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|input| input.ends_with("punctual-rig.gltf"))
    );
    let generated = punctual_lights_ui::PunctualLightsDemo::new();
    let weak = generated.as_weak();
    let dynamic = program.create("PunctualLightsDemo")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy)]
    struct View {
        time: f64,
        camera_x: f64,
        rig_yaw: f64,
        visible: bool,
        imported: bool,
        manual: bool,
        shadows: bool,
    }
    let base = View {
        time: 0.0,
        camera_x: 0.0,
        rig_yaw: 0.0,
        visible: true,
        imported: true,
        manual: false,
        shadows: false,
    };
    let mut render = |view: View| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing lights demo")?;
        app.set_time(view.time);
        app.set_camera_x(view.camera_x);
        app.set_rig_yaw(view.rig_yaw);
        app.set_rig_visible(view.visible);
        app.set_imported(view.imported);
        app.set_manual(view.manual);
        app.set_shadows(view.shadows);
        for (name, value) in [
            ("time", Value::Float(view.time)),
            ("camera-x", Value::Float(view.camera_x)),
            ("rig-yaw", Value::Float(view.rig_yaw)),
            ("rig-visible", Value::Bool(view.visible)),
            ("imported", Value::Bool(view.imported)),
            ("manual", Value::Bool(view.manual)),
            ("shadows", Value::Bool(view.shadows)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert_eq!(
            after.color_instances - before.color_instances,
            2,
            "lights-only asset creates no geometry"
        );
        assert_eq!(
            after.color_draws - before.color_draws,
            1,
            "boxes share one batch"
        );
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        let read = |preview: &PreviewScene| -> TestResult<Option<std::sync::Arc<[SceneLight]>>> {
            let (node, _) = inspect(preview)?;
            let Some(Value::SceneObjects(scene)) = node.properties.get("model") else {
                return Err("missing scene model".into());
            };
            Ok(scene.imported_lights())
        };
        let lights = read(&aot)?;
        assert_eq!(lights, read(&interpreted)?);
        Ok((
            xrack_assets::decode_png(&png)?,
            lights,
            renderer.scene_draw_stats().light_buffer_uploads,
            renderer.scene_shadow_passes(),
        ))
    };

    let (start, lights, uploads, _) = render(base)?;
    let lights = lights.ok_or("expected imported lights")?;
    assert_eq!(lights.len(), 3);
    let near = |a: f32, b: f32| assert!((a - b).abs() <= 1e-5, "{a} != {b}");
    let SceneLightSource::Point {
        position,
        inverse_range_squared,
    } = lights[0].source
    else {
        return Err("expected point light".into());
    };
    for (a, b) in position.into_iter().zip([-2.86, 0.27, 2.625]) {
        near(a, b);
    }
    near(inverse_range_squared, 0.04);
    for (a, b) in lights[0].radiance.into_iter().zip([21.6, 1.2, 0.48]) {
        near(a, b);
    }
    let SceneLightSource::Spot {
        toward_light,
        inverse_range_squared,
        outer_cos,
        ..
    } = lights[1].source
    else {
        return Err("expected spot light".into());
    };
    near(toward_light[0], 20.0f32.to_radians().sin());
    near(toward_light[2], 20.0f32.to_radians().cos());
    near(inverse_range_squared, 0.0625);
    near(outer_cos, 30.0f32.to_radians().cos());
    let SceneLightSource::Directional { toward_light } = lights[2].source else {
        return Err("expected directional fill".into());
    };
    near(toward_light[0], (-12.0f32).to_radians().sin());
    near(toward_light[2], 12.0f32.to_radians().cos());

    let (_, shifted_lights, shifted_uploads, _) = render(View {
        camera_x: 0.22,
        ..base
    })?;
    assert_eq!(shifted_lights.as_deref(), Some(lights.as_ref()));
    assert_eq!(
        shifted_uploads, uploads,
        "camera movement reuses the light buffer"
    );
    let (end, animated, animated_uploads, _) = render(View { time: 2.0, ..base })?;
    let animated = animated.ok_or("expected animated lights")?;
    let SceneLightSource::Point { position, .. } = animated[0].source else {
        return Err("missing moving point".into());
    };
    near(position[0], 2.99);
    let SceneLightSource::Spot { toward_light, .. } = animated[1].source else {
        return Err("missing sweeping cone".into());
    };
    near(toward_light[0], -20.0f32.to_radians().sin());
    assert!(animated_uploads > uploads);

    let (_, turned, _, _) = render(View {
        rig_yaw: 30.0,
        ..base
    })?;
    let turned = turned.ok_or("expected reoriented lights")?;
    let SceneLightSource::Spot { toward_light, .. } = turned[1].source else {
        return Err("missing reoriented spot".into());
    };
    near(toward_light[0], 50.0f32.to_radians().sin());

    let (off, _, _, _) = render(View {
        imported: false,
        ..base
    })?;
    let (hidden, absent, _, _) = render(View {
        visible: false,
        ..base
    })?;
    assert!(absent.is_none());
    let view_bytes = 460 * 222 * 4;
    assert_eq!(&off.rgba[..view_bytes], &hidden.rgba[..view_bytes]);
    let (mixed, _, _, _) = render(View {
        manual: true,
        ..base
    })?;
    let (_, _, _, shadows) = render(View {
        shadows: true,
        ..base
    })?;
    assert!(shadows > 0);
    let sample = |frame: &xrack_assets::Raster, x: usize, y: usize| {
        let start = 4 * (460 * y + x);
        [
            frame.rgba[start],
            frame.rgba[start + 1],
            frame.rgba[start + 2],
        ]
    };
    let start_red = [sample(&start, 120, 111)[0], sample(&start, 348, 111)[0]];
    let end_red = [sample(&end, 120, 111)[0], sample(&end, 348, 111)[0]];
    assert!(start_red[0] > start_red[1].saturating_add(40));
    assert!(end_red[1] > end_red[0].saturating_add(40));
    assert!(sample(&mixed, 120, 111)[1] > sample(&start, 120, 111)[1].saturating_add(15));
    assert_eq!(sample(&off, 120, 111), [0; 3]);
    eprintln!(
        "imported animated point red [left, right] {start_red:?} → {end_red:?}; {shadows} shadow maps"
    );
    Ok(())
}

#[test]
fn dispersive_refraction_samples_three_paths_without_an_extra_capture() -> TestResult {
    const NAME: &str = "examples/puzzle3d/dispersion.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("DispersionDemo")?;
    let app = dispersion_ui::DispersionDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    let mut render = |dispersed, yaw, msaa, show_glass| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing dispersion demo")?;
        app.set_dispersed(dispersed);
        app.set_yaw(yaw);
        app.set_msaa(msaa);
        app.set_show_glass(show_glass);
        for (name, value) in [
            ("dispersed", Value::Bool(dispersed)),
            ("yaw", Value::Float(yaw)),
            ("msaa", Value::Bool(msaa)),
            ("show-glass", Value::Bool(show_glass)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        assert_eq!(
            after.color_draws - before.color_draws,
            1 + u64::from(show_glass)
        );
        assert_eq!(
            after.transmission_captures - before.transmission_captures,
            u64::from(show_glass)
        );
        assert_eq!(
            after.volume_depth_captures - before.volume_depth_captures,
            u64::from(show_glass)
        );
        assert_eq!(
            after.transmission_mip_passes - before.transmission_mip_passes,
            0
        );
        assert_eq!(
            after.multisample_resolves - before.multisample_resolves,
            u64::from(msaa)
        );
        Ok(xrack_assets::decode_png(&png)?)
    };
    let background = render(false, 32.0, false, false)?;
    let reference = render(false, 32.0, false, true)?;
    let dispersed = render(true, 32.0, false, true)?;
    let msaa = render(true, 32.0, true, true)?;
    let face_on = render(true, 0.0, false, true)?;
    let face_reference = render(false, 0.0, false, true)?;

    let pixel = |image: &xrack_assets::Raster, x: usize, y: usize| -> [u8; 3] {
        let start = 4 * (460 * y + x);
        [
            image.rgba[start],
            image.rgba[start + 1],
            image.rgba[start + 2],
        ]
    };
    let linear = |value: u8| {
        let srgb = f64::from(value) / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    let encoded = |value: f64| {
        (255.0
            * if value <= 0.0031308 {
                value * 12.92
            } else {
                1.055 * value.powf(1.0 / 2.4) - 0.055
            })
        .round() as u8
    };
    let (sin, cos) = 32.0_f64.to_radians().sin_cos();
    let shifts = [1.36_f64, 1.6, 1.84].map(|ior| {
        let eta = ior.recip();
        let coefficient = eta * cos - (1.0 - (eta * sin).powi(2)).sqrt();
        let dx = coefficient * sin;
        let dz = -eta + coefficient * cos;
        let mesh_distance = ((cos * dx - sin * dz) / 3.6).hypot((sin * dx + cos * dz) / 2.3);
        dx / mesh_distance * 40.0
    });
    let mut separated = 0;
    let mut separated_msaa = 0;
    for (x, y) in (85..140).flat_map(|y| (205..291).map(move |x| (x, y))) {
        assert_eq!(pixel(&face_on, x, y), pixel(&face_reference, x, y));
        let neutral = pixel(&reference, x, y);
        assert_eq!(neutral, [neutral[1]; 3]);
        let split = pixel(&dispersed, x, y);
        assert_eq!(neutral[1], split[1], "the green ray keeps the authored IOR");
        let expected = shifts.map(|shift| {
            let position = x as f64 + shift;
            let left = position.floor() as usize;
            let a = linear(pixel(&background, left, y)[0]);
            let b = linear(pixel(&background, left + 1, y)[0]);
            encoded(a + (b - a) * position.fract())
        });
        assert!(
            split
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "RGB at {x}/{y}: {split:?}, expected {expected:?}"
        );
        separated += usize::from(split != [split[1]; 3]);
        let multisampled = pixel(&msaa, x, y);
        separated_msaa += usize::from(multisampled != [multisampled[1]; 3]);
    }
    assert!(separated > 1_000);
    assert!(separated_msaa > 1_000);
    eprintln!(
        "RGB shifts {shifts:?}; {separated} single-sample and {separated_msaa} MSAA dispersed pixels"
    );
    Ok(())
}

#[test]
fn volume_glass_projects_refracted_rays_and_attenuates_in_world_units() -> TestResult {
    const NAME: &str = "examples/puzzle3d/volume.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("VolumeDemo")?;
    let app = volume_ui::VolumeDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    #[derive(Clone, Copy, PartialEq)]
    enum Occlusion {
        None,
        Cover,
        RefractionBlocker,
    }
    let mut render = |depth: f64,
                      yaw: f64,
                      volume: bool,
                      patterned: bool,
                      occlusion: Occlusion,
                      msaa: bool|
     -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing volume example")?;
        let occlude = occlusion == Occlusion::Cover;
        let blocker = occlusion == Occlusion::RefractionBlocker;
        app.set_depth_scale(depth);
        app.set_yaw(yaw);
        app.set_volume(volume);
        app.set_patterned(patterned);
        app.set_occlude(occlude);
        app.set_refraction_blocker(blocker);
        app.set_msaa(msaa);
        for (name, value) in [
            ("depth-scale", Value::Float(depth)),
            ("yaw", Value::Float(yaw)),
            ("volume", Value::Bool(volume)),
            ("patterned", Value::Bool(patterned)),
            ("occlude", Value::Bool(occlude)),
            ("refraction-blocker", Value::Bool(blocker)),
            ("msaa", Value::Bool(msaa)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        assert_eq!(
            after.transmission_captures - before.transmission_captures,
            1
        );
        assert_eq!(
            after.transmission_mip_passes - before.transmission_mip_passes,
            0
        );
        assert_eq!(
            after.color_draws - before.color_draws,
            (if volume { 2 } else { 1 })
                + u64::from(patterned)
                + u64::from(occlusion != Occlusion::None)
        );
        assert_eq!(
            after.volume_depth_captures - before.volume_depth_captures,
            u64::from(volume)
        );
        Ok(xrack_assets::decode_png(&png)?)
    };

    let initial = render(1.0, 0.0, true, false, Occlusion::None, false)?;
    let stretched = render(1.5, 0.0, true, false, Occlusion::None, false)?;
    let pixel = |frame: &xrack_assets::Raster, x: usize, y: usize| {
        let start = 4 * (460 * y + x);
        [
            frame.rgba[start],
            frame.rgba[start + 1],
            frame.rgba[start + 2],
        ]
    };
    let srgb = |linear: f64| -> u8 {
        (255.0
            * if linear <= 0.0031308 {
                12.92 * linear
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            })
        .round() as u8
    };
    for (frame, scale) in [(&initial, 1.0), (&stretched, 1.5)] {
        for (x, path) in [
            (144, 64.0 / 255.0),
            (184, 1.0),
            (276, 2.0 * 64.0 / 255.0),
            (316, 2.0),
        ] {
            let expected = [0.25_f64, 0.5, 0.8].map(|channel| srgb(channel.powf(path * scale)));
            let sampled = pixel(frame, x, 112);
            assert!(
                sampled
                    .into_iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 2),
                "volume thicken at x={x} scale={scale}: {sampled:?} expected {expected:?}"
            );
        }
    }
    let volume = render(1.0, 24.0, true, true, Occlusion::None, false)?;
    let thin = render(1.0, 24.0, false, true, Occlusion::None, false)?;
    let shifted = (88..138)
        .flat_map(|y| (133..172).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&volume, x, y)[0] > pixel(&thin, x, y)[0].saturating_add(20))
        .count();
    assert!(
        shifted > 200,
        "only a displaced ray would brighten {shifted} previously dark pixels"
    );

    let linear = |channel: u8| -> f64 {
        let encoded = f64::from(channel) / 255.0;
        if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    };
    let (sin, cos) = 24.0_f64.to_radians().sin_cos();
    let eta = 2.0 / 3.0;
    let coefficient = eta * cos - (1.0 - eta * eta * sin * sin).sqrt();
    let dx = coefficient * sin;
    let dz = -eta + coefficient * cos;
    let local_x = (cos * dx - sin * dz) * 0.5;
    let local_z = sin * dx + cos * dz;
    let path = local_x.hypot(local_z).recip();
    let attenuation = [0.25_f64, 0.5, 0.8].map(|color| color.powf(path));
    for msaa in [false, true] {
        let guarded = render(1.0, 24.0, true, true, Occlusion::RefractionBlocker, msaa)?;
        let guarded_count = (103..120)
            .flat_map(|y| (193..197).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let reference = pixel(&thin, x, y);
                let entry =
                    std::array::from_fn::<_, 3, _>(|i| srgb(linear(reference[i]) * attenuation[i]));
                let protected = pixel(&guarded, x, y);
                let displaced = pixel(&volume, x, y);
                protected
                    .into_iter()
                    .zip(entry)
                    .all(|(a, b)| a.abs_diff(b) <= 3)
                    && displaced
                        .into_iter()
                        .zip(entry)
                        .any(|(a, b)| a.abs_diff(b) > 20)
            })
            .count();
        assert!(
            guarded_count >= 5,
            "MSAA {msaa} protected {guarded_count} glass pixels from the foreground"
        );
        eprintln!(
            "volume depth guard fell back to the entry pixel {guarded_count} times with MSAA={msaa}"
        );
    }

    let cover_volume = render(1.0, 0.0, true, false, Occlusion::Cover, false)?;
    let cover_thin = render(1.0, 0.0, false, false, Occlusion::Cover, false)?;
    assert_eq!(pixel(&cover_volume, 174, 112), pixel(&cover_thin, 174, 112));
    assert_ne!(pixel(&cover_volume, 174, 112), pixel(&initial, 174, 112));
    eprintln!(
        "volume red at two glTF node scales: {:?}, thicker instance: {:?}; refracted pixels {shifted}",
        [pixel(&initial, 184, 112)[0], pixel(&initial, 316, 112)[0]],
        [
            pixel(&stretched, 184, 112)[0],
            pixel(&stretched, 316, 112)[0]
        ]
    );
    Ok(())
}

#[test]
fn thin_transmission_keeps_coverage_and_reuses_opaque_color_in_both_backends() -> TestResult {
    const NAME: &str = "examples/puzzle3d/transmission.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("TransmissionDemo")?;
    let app = transmission_ui::TransmissionDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    #[derive(Clone, Copy)]
    struct View {
        clear: bool,
        frost: bool,
        overlay: bool,
        optical_overlay: bool,
        weighted: bool,
        hdr: bool,
        msaa: bool,
        scale: f64,
    }
    let baseline = View {
        clear: false,
        frost: false,
        overlay: false,
        optical_overlay: false,
        weighted: true,
        hdr: false,
        msaa: false,
        scale: 1.0,
    };
    let mut render = |view: View| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing transmission demo")?;
        app.set_clear_glass(view.clear);
        app.set_frost(view.frost);
        app.set_overlay(view.overlay);
        app.set_optical_overlay(view.optical_overlay);
        app.set_weighted(view.weighted);
        app.set_hdr(view.hdr);
        app.set_msaa(view.msaa);
        app.set_aa_scale(view.scale);
        for (name, value) in [
            ("clear-glass", Value::Bool(view.clear)),
            ("frost", Value::Bool(view.frost)),
            ("overlay", Value::Bool(view.overlay)),
            ("optical-overlay", Value::Bool(view.optical_overlay)),
            ("weighted", Value::Bool(view.weighted)),
            ("hdr", Value::Bool(view.hdr)),
            ("msaa", Value::Bool(view.msaa)),
            ("aa-scale", Value::Float(view.scale)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        assert_eq!(
            after.transmission_captures - before.transmission_captures,
            u64::from(view.clear || view.frost || (view.overlay && view.optical_overlay))
        );
        assert_eq!(
            after.transmission_mip_passes - before.transmission_mip_passes,
            if view.frost { 6 } else { 0 }
        );
        assert_eq!(
            after.transparency_composites - before.transparency_composites,
            u64::from(view.overlay && view.weighted)
        );
        assert_eq!(
            after.multisample_resolves - before.multisample_resolves,
            u64::from(view.msaa && view.scale == 1.0)
        );
        assert_eq!(
            after.supersample_resolves - before.supersample_resolves,
            u64::from(view.scale > 1.0)
        );
        assert_eq!(
            after.hdr_resolves - before.hdr_resolves,
            u64::from(view.hdr)
        );
        assert_eq!(
            after.color_draws - before.color_draws,
            1 + u64::from(view.clear) + u64::from(view.frost) + u64::from(view.overlay)
        );
        Ok((
            xrack_assets::decode_png(&png)?,
            renderer.scene_mesh_uploads(),
        ))
    };

    let (opaque, _) = render(baseline)?;
    let (clear, _) = render(View {
        clear: true,
        ..baseline
    })?;
    let full = View {
        clear: true,
        frost: true,
        ..baseline
    };
    let (frost, _) = render(full)?;
    let (combined, uploads) = render(View {
        msaa: true,
        hdr: true,
        overlay: true,
        ..full
    })?;
    let (supersampled, later_uploads) = render(View {
        scale: 1.5,
        hdr: true,
        overlay: true,
        ..full
    })?;
    assert_eq!(uploads, later_uploads);
    assert_ne!(combined.rgba, supersampled.rgba);
    let transparent = View {
        overlay: true,
        optical_overlay: true,
        ..baseline
    };
    let (weighted, _) = render(transparent)?;
    let (sorted, _) = render(View {
        weighted: false,
        ..transparent
    })?;
    let _ = render(full)?;
    let pixel = |frame: &xrack_assets::Raster, x: usize, y: usize| {
        let start = 4 * (480 * y + x);
        [
            frame.rgba[start],
            frame.rgba[start + 1],
            frame.rgba[start + 2],
        ]
    };
    for y in [83, 125, 152] {
        assert_eq!(pixel(&clear, 105, y), pixel(&opaque, 105, y));
        assert_ne!(pixel(&clear, 165, y), pixel(&opaque, 165, y));
        assert_eq!(pixel(&frost, 105, y), pixel(&clear, 105, y));
    }
    for x in [295, 320, 350] {
        assert_eq!(
            pixel(&frost, x, 85),
            pixel(&opaque, x, 85),
            "alpha mask leaves the top half clear"
        );
    }
    let original = pixel(&opaque, 239, 120);
    let covered = pixel(&sorted, 239, 120);
    let oit = pixel(&weighted, 239, 120);
    assert!(original[0].abs_diff(covered[0]) <= 1);
    assert!(original[1].saturating_sub(covered[1]) > 5);
    assert!(
        covered
            .into_iter()
            .zip(oit)
            .all(|(a, b)| a.abs_diff(b) <= 2)
    );

    use xrack_runtime::{Length, Point2D, scene3d::pick_camera};
    for preview in [&aot, &interpreted] {
        let (view, _) = inspect(preview)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing transmission scene".into());
        };
        let viewport = Point2D::new(Length(480.0), Length(240.0));
        let pick = |x, y| {
            pick_camera(
                scene,
                "eye",
                viewport,
                Point2D::new(Length(x), Length(y)),
                None,
            )
        };
        assert_eq!(pick(105.0, 83.0), "clear");
        assert_eq!(pick(330.0, 85.0), "wall");
        assert_eq!(pick(330.0, 154.0), "frost");
    }
    let contrast = |frame: &xrack_assets::Raster| -> u64 {
        (137..174)
            .flat_map(|y| (297..369).map(move |x| (x, y)))
            .map(|(x, y)| u64::from(pixel(frame, x, y)[0].abs_diff(pixel(frame, x + 1, y)[0])))
            .sum()
    };
    let before = contrast(&opaque);
    let after = contrast(&frost);
    assert!(
        after * 5 < before,
        "frosted checkerboard contrast {before} → {after}"
    );
    eprintln!(
        "thin transmission checkerboard contrast {before} → {after}, frost sample {:?}",
        pixel(&frost, 330, 154)
    );
    Ok(())
}

#[test]
fn dielectric_materials_apply_ior_specular_maps_and_metallic_weight_in_both_backends() -> TestResult
{
    const NAME: &str = "examples/puzzle3d/dielectric.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, resources) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        resources
            .iter()
            .any(|resource| resource.ends_with("dielectric.gltf"))
    );
    let compiled = xrack_compiler::compile_preview(&source, NAME, Default::default())?;
    assert_eq!(
        compiled
            .rust
            .to_string()
            .matches("MeshImage :: from_packed")
            .count(),
        1
    );
    let dynamic = program.create("DielectricDemo")?;
    let app = dielectric_ui::DielectricDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    let mut render = |studio: bool, yaw: f64| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing dielectric demo")?;
        app.set_studio(studio);
        app.set_light_yaw(yaw);
        dynamic.set("studio", Value::Bool(studio))?;
        dynamic.set("light-yaw", Value::Float(yaw))?;
        let before = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        assert_eq!(after.color_draws - before.color_draws, 8);
        assert_eq!(after.hdr_resolves - before.hdr_resolves, u64::from(studio));
        Ok((
            xrack_assets::decode_png(&png)?,
            renderer.scene_mesh_uploads(),
        ))
    };
    let (direct, uploads) = render(false, 0.0)?;
    let (rotated, next_uploads) = render(false, 15.0)?;
    let (probe, probe_uploads) = render(true, 0.0)?;
    assert_eq!(uploads, next_uploads);
    assert_eq!(uploads, probe_uploads);
    let pixel = |frame: &xrack_assets::Raster, x: usize, y: usize| -> [u8; 3] {
        let start = 4 * (480 * y + x);
        [
            frame.rgba[start],
            frame.rgba[start + 1],
            frame.rgba[start + 2],
        ]
    };
    let compare =
        |frame: &xrack_assets::Raster| [72, 184, 296, 408].map(|x| pixel(frame, x, 62)[0]);
    let ior = compare(&direct);
    let environment = compare(&probe);
    eprintln!(
        "IOR [1.5, 1.0, 2.42, 0] under direct and environment lighting: {ior:?}, {environment:?}"
    );
    for brightness in [ior, environment] {
        assert!(brightness[1] < brightness[0]);
        assert!(brightness[0] < brightness[2]);
        assert!(brightness[2] < brightness[3]);
    }
    assert_ne!(compare(&rotated), ior);
    for frame in [&direct, &probe] {
        let baseline = pixel(frame, 72, 62);
        let transparent_strength = pixel(frame, 52, 178);
        let colored = pixel(frame, 92, 178);
        let tint = pixel(frame, 184, 178);
        let total = |rgb: [u8; 3]| rgb.into_iter().map(u32::from).sum::<u32>();
        eprintln!(
            "specular map [zero strength, tinted, white reference]: {transparent_strength:?}, {colored:?}, {baseline:?}"
        );
        assert!(total(colored) > total(transparent_strength) + 20);
        // The HDR panorama is red-biased. The blue tint changes the B/R ratio
        // relative to the untinted material under the same radiance.
        assert!(
            u16::from(colored[2]) * u16::from(baseline[0])
                > u16::from(colored[0]) * u16::from(baseline[2])
        );
        assert!(tint[0] > tint[1].saturating_add(8), "{tint:?}");
        assert_eq!(
            pixel(frame, 296, 178),
            pixel(frame, 408, 178),
            "metallic=1 bypasses dielectric reflectance"
        );
    }
    let (node, _) = inspect(&aot)?;
    let Some(Value::SceneObjects(scene)) = node.properties.get("model") else {
        return Err("missing dielectric objects".into());
    };
    let dielectric = scene[0].mesh.materials()[4]
        .pbr
        .as_ref()
        .and_then(|pbr| pbr.dielectric.as_ref())
        .ok_or("missing specular maps")?;
    let strength = dielectric
        .specular_texture
        .as_ref()
        .ok_or("missing alpha map")?;
    let color = dielectric
        .color_texture
        .as_ref()
        .ok_or("missing color map")?;
    assert!(std::sync::Arc::ptr_eq(&strength.image, &color.image));
    assert_ne!(strength.transform, color.transform);
    Ok(())
}

#[test]
fn gltf_uv_transforms_keep_masked_picking_and_normal_lighting_in_sync() -> TestResult {
    use xrack_runtime::scene3d::Point3D;

    const NAME: &str = "examples/puzzle3d/uv_atlas.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let compiled = xrack_compiler::compile_preview(&source, NAME, Default::default())?;
    assert_eq!(
        compiled
            .rust
            .to_string()
            .matches("MeshImage :: from_packed")
            .count(),
        2
    );
    let dynamic = program.create("UvAtlasDemo")?;
    let app = uv_atlas_ui::UvAtlasDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut render = |yaw, shadows, screen| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing atlas demo")?;
        app.set_light_yaw(yaw);
        app.set_shadows(shadows);
        app.set_shadow_screen(screen);
        dynamic.set("light-yaw", Value::Float(yaw))?;
        dynamic.set("shadows", Value::Bool(shadows))?;
        dynamic.set("shadow-screen", Value::Bool(screen))?;
        let png = generated.render(&mut renderer)?.to_vec();
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        let (view, _) = inspect(&generated)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing UV scene".into());
        };
        let camera = scene
            .camera("eye", [400., 230.])
            .ok_or("missing atlas camera")?;
        Ok((xrack_assets::decode_png(&png)?, camera))
    };
    let (lit_left, camera) = render(-40.0, false, false)?;
    let (lit_right, _) = render(40.0, false, false)?;
    let (shadowed, _) = render(-40.0, true, false)?;
    let sample = |image: &xrack_assets::Raster, point: Point3D| {
        let pixel = camera.project(point).point;
        let index = 4 * (pixel.y.0 as usize * 400 + pixel.x.0 as usize);
        [
            image.rgba[index],
            image.rgba[index + 1],
            image.rgba[index + 2],
        ]
    };
    let background = [0x17, 0x23, 0x33];
    let red = [237, 58, 51];
    let green = [50, 207, 105];
    let blue = [52, 108, 242];
    let quadrants = [[-0.5, 0.5], [0.5, 0.5], [-0.5, -0.5], [0.5, -0.5]];
    for (center, expected) in [
        (-1.35, [red, green, blue, background]),
        (1.35, [background, green, blue, red]),
    ] {
        for ([dx, dy], color) in quadrants.into_iter().zip(expected) {
            let point = Point3D::new(center + dx, 1.2 + dy, 0.0);
            assert_eq!(sample(&lit_left, point), color);
            assert_eq!(sample(&shadowed, point), color);
            let cursor = camera.project(point).point;
            let mut app = weak.upgrade().ok_or("missing atlas demo")?;
            app.set_pointer(cursor);
            dynamic.set("pointer", Value::Point2D(cursor))?;
            let hit = if color == background { "" } else { "atlas" };
            assert_eq!(app.get_picked(), hit);
            assert_eq!(dynamic.get("picked")?, Value::String(hit.into()));
        }
    }

    let matte_left = Point3D::new(-1.35, -1.2, 0.0);
    let matte_right = Point3D::new(1.35, -1.2, 0.0);
    let brightness = [
        [
            sample(&lit_left, matte_left)[0],
            sample(&lit_left, matte_right)[0],
        ],
        [
            sample(&lit_right, matte_left)[0],
            sample(&lit_right, matte_right)[0],
        ],
    ];
    eprintln!("mirrored normal map under opposing lights: {brightness:?}");
    assert!(brightness[0][0].abs_diff(brightness[0][1]) > 25);
    assert_ne!(
        brightness[0][0] > brightness[0][1],
        brightness[1][0] > brightness[1][1]
    );
    let (lit_screen, _) = render(-40.0, false, true)?;
    let (cast_screen, _) = render(-40.0, true, true)?;
    assert!(renderer.scene_shadow_passes() > 0);
    let projected_x = 0.85 + 2.0 * 40.0_f64.to_radians().tan();
    let behind_hole = Point3D::new(projected_x, 1.7, -2.0);
    let behind_blue = Point3D::new(projected_x, 0.7, -2.0);
    let lit = [
        sample(&lit_screen, behind_hole)[0],
        sample(&lit_screen, behind_blue)[0],
    ];
    let cast = [
        sample(&cast_screen, behind_hole)[0],
        sample(&cast_screen, behind_blue)[0],
    ];
    eprintln!("receiver after transformed cutout shadow: {lit:?} → {cast:?}");
    assert!(lit[0].abs_diff(cast[0]) <= 2);
    assert!(lit[1].saturating_sub(cast[1]) > 40);
    Ok(())
}

#[test]
fn gltf_mipmaps_smooth_minified_textures_without_warm_uploads() -> TestResult {
    use xrack_runtime::scene3d::{Point3D, TextureColorSpace, TextureFilter};

    const NAME: &str = "examples/puzzle3d/texture_mips.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|input| input.ends_with("minification-grid.png"))
    );
    let compiled = xrack_compiler::compile_preview(&source, NAME, Default::default())?;
    assert_eq!(
        compiled
            .rust
            .to_string()
            .matches("MeshImage :: from_packed")
            .count(),
        1
    );
    let dynamic = program.create("TextureMipmapDemo")?;
    let app = texture_mips_ui::TextureMipmapDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut initial_uploads = None;
    for (distance, yaw) in [(18.0, 0.0), (22.0, 20.0), (25.0, -25.0)] {
        let mut app = weak.upgrade().ok_or("missing generated mipmap demo")?;
        app.set_distance(distance);
        app.set_yaw(yaw);
        dynamic.set("distance", Value::Float(distance))?;
        dynamic.set("yaw", Value::Float(yaw))?;
        let before = renderer.scene_draw_stats();
        let png = generated.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert_eq!(after.color_draws - before.color_draws, 2);
        assert_eq!(after.supersample_resolves, before.supersample_resolves);
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        let uploaded = renderer.scene_draw_stats().material_mip_levels_uploaded;
        assert_eq!(uploaded, *initial_uploads.get_or_insert(uploaded));
        assert_eq!(uploaded, 12, "one six-level upload for each backend");

        let camera = SceneCamera::new(yaw, 0.0, distance, [400.0, 200.0]).ok_or("camera")?;
        let frame = xrack_assets::decode_png(&png)?;
        let spread = |x| {
            let center = camera.project(Point3D::new(x, 0.0, 0.0)).point.x.0 as usize;
            let rows = (93..107).flat_map(|y| (center - 7..center + 7).map(move |x| (x, y)));
            rows.map(|(x, y)| frame.rgba[4 * (400 * y + x)])
                .fold((255u8, 0u8), |(min, max), value| {
                    (min.min(value), max.max(value))
                })
        };
        let nearest = spread(-2.05);
        let trilinear = spread(2.05);
        eprintln!(
            "texture minification at {distance}/{yaw}: nearest {nearest:?}, mipmapped {trilinear:?}"
        );
        assert!(nearest.0 < 20 && nearest.1 > 230);
        assert!(trilinear.0 >= 183 && trilinear.1 <= 192);
    }
    for scene in [&generated, &interpreted] {
        let (node, _) = inspect(scene)?;
        let Some(Value::SceneObjects(objects)) = node.properties.get("model") else {
            return Err("missing mipmap test objects".into());
        };
        let materials = objects[0].mesh.materials();
        let texture = |index: usize| {
            materials[index]
                .base_texture
                .as_ref()
                .ok_or("missing card texture")
        };
        let source = texture(0)?;
        let mipped = texture(1)?;
        assert_eq!(source.mip_filter, None);
        assert_eq!(mipped.mip_filter, Some(TextureFilter::Linear));
        assert!(std::sync::Arc::ptr_eq(&source.image, &mipped.image));
        assert_eq!(
            mipped
                .image
                .prepared_mips(TextureColorSpace::Srgb)
                .map(<[u8]>::len),
            Some(10_668)
        );
        assert!(
            mipped
                .image
                .prepared_mips(TextureColorSpace::Linear)
                .is_none()
        );
    }
    Ok(())
}

#[test]
fn specular_aa_stabilizes_moving_highlights_and_preserves_unlit_pixels() -> TestResult {
    const NAME: &str = "examples/puzzle3d/specular_aa.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("SpecularDemo")?;
    let app = specular_aa_ui::SpecularDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    let mut render = |aa, studio, msaa, yaw| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing specular AA demo")?;
        app.set_filtered(aa);
        app.set_studio(studio);
        app.set_msaa(msaa);
        app.set_yaw(yaw);
        for (name, value) in [
            ("filtered", Value::Bool(aa)),
            ("studio", Value::Bool(studio)),
            ("msaa", Value::Bool(msaa)),
            ("yaw", Value::Float(yaw)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let png = aot.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(png.as_slice() == interpreted.render(&mut renderer)?);
        assert_eq!(app.get_picked(), "spheres");
        assert_eq!(dynamic.get("picked")?, Value::String("spheres".into()));
        assert_eq!(after.color_draws - before.color_draws, 3);
        assert_eq!(
            after.transparency_composites - before.transparency_composites,
            1
        );
        assert_eq!(after.hdr_resolves - before.hdr_resolves, 1);
        assert_eq!(
            after.multisample_resolves - before.multisample_resolves,
            u64::from(msaa)
        );
        assert_eq!(after.supersample_resolves, before.supersample_resolves);
        Ok(xrack_assets::decode_png(&png)?)
    };
    let region = |frame: &xrack_assets::Raster, center: usize| -> Vec<u8> {
        (78..122)
            .flat_map(|y| (center - 17..center + 17).map(move |x| (x, y)))
            .map(|(x, y)| frame.rgba[4 * (400 * y + x)])
            .collect()
    };

    let mut peaks = [Vec::new(), Vec::new()];
    for step in 0..12 {
        let yaw = -4.0 + 0.71 * f64::from(step);
        let sharp = render(false, false, false, yaw)?;
        let filtered = render(true, false, false, yaw)?;
        for (sample, image) in peaks.iter_mut().zip([&sharp, &filtered]) {
            let max_red = |center| region(image, center).into_iter().max().unwrap_or(0);
            sample.push([max_red(133), max_red(200)]);
        }
        assert_eq!(region(&sharp, 267), region(&filtered, 267));
    }
    let variation = peaks.map(|values| {
        [0, 1].map(|column| {
            values
                .windows(2)
                .map(|pair| u32::from(pair[0][column].abs_diff(pair[1][column])))
                .sum::<u32>()
        })
    });
    eprintln!("highlight peak variation [metal, clearcoat]: {variation:?}");
    for (sharp, filtered) in variation[0].into_iter().zip(variation[1]) {
        assert!(filtered < sharp, "{sharp} → {filtered}");
    }

    let probe = render(false, true, true, 0.7)?;
    let filtered_probe = render(true, true, true, 0.7)?;
    assert_ne!(region(&probe, 133), region(&filtered_probe, 133));
    assert_ne!(region(&probe, 200), region(&filtered_probe, 200));
    assert_eq!(region(&probe, 267), region(&filtered_probe, 267));
    Ok(())
}

#[test]
fn local_fog_scatters_punctual_lights_and_reuses_resident_shadow_geometry() -> TestResult {
    use fog_lighting_ui::FogLightingDemo;
    use xrack_runtime::scene3d::Point3D;

    const NAME: &str = "examples/puzzle3d/fog_lighting.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("FogLightingDemo")?;
    let app = FogLightingDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let camera = SceneCamera::new(0.0, 0.0, 10.0, [600.0, 356.0]).ok_or("missing camera")?;
    let behind = camera.project(Point3D::new(1.25, 0.0, 0.0)).point;
    let side = camera.project(Point3D::new(1.25, 1.65, 0.0)).point;
    let left = camera.project(Point3D::new(-2.4, 0.0, 0.0)).point;
    let sample = |pixels: &[u8], point: xrack_runtime::Point2D| {
        let x = point.x.0 as usize;
        let y = point.y.0 as usize;
        let sum = (y - 3..y + 4)
            .flat_map(|row| (x - 3..x + 4).map(move |col| (row, col)))
            .map(|(row, col)| u32::from(pixels[4 * (600 * row + col)]))
            .sum::<u32>();
        sum as f32 / 49.0
    };
    let render = |aot: &mut PreviewScene,
                  interpreter: &mut PreviewScene,
                  gpu: &mut Renderer,
                  other_gpu: &mut Renderer|
     -> TestResult<Vec<u8>> {
        let png = aot.render(gpu)?.to_vec();
        assert!(png.as_slice() == interpreter.render(other_gpu)?);
        Ok(xrack_assets::decode_png(&png)?.rgba[..600 * 356 * 4].to_vec())
    };
    let mut app = weak.upgrade().ok_or("missing lit fog demo")?;
    app.set_scattering(false);
    app.set_shadow(false);
    dynamic.set("scattering", Value::Bool(false))?;
    dynamic.set("shadow", Value::Bool(false))?;
    let unlit = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    let mesh_uploads = gpu.scene_mesh_uploads();
    let instance_uploads = gpu.scene_instance_uploads();
    let draws = gpu.scene_draw_stats().color_draws;

    app.set_scattering(true);
    dynamic.set("scattering", Value::Bool(true))?;
    let beam = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(sample(&beam, behind) > sample(&unlit, behind) + 10.0);
    assert!(sample(&beam, left) > sample(&beam, behind));
    assert_eq!(gpu.scene_draw_stats().color_draws - draws, draws);

    app.set_shadow(true);
    dynamic.set("shadow", Value::Bool(true))?;
    let shadowed = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    let depth_passes = gpu.scene_shadow_passes();
    assert!(depth_passes > 0);
    assert!(sample(&shadowed, behind) + 10.0 < sample(&beam, behind));
    assert!((sample(&shadowed, left) - sample(&beam, left)).abs() < 2.0);
    assert!(sample(&shadowed, side) > sample(&unlit, side));
    assert_eq!(gpu.scene_instance_uploads(), instance_uploads);

    app.set_density(0.28);
    dynamic.set("density", Value::Float(0.28))?;
    let thinner = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert_ne!(sample(&thinner, behind), sample(&shadowed, behind));
    assert_eq!(gpu.scene_shadow_passes(), depth_passes);
    assert_eq!(gpu.scene_mesh_uploads(), mesh_uploads);

    app.set_lamp_y(1.05);
    dynamic.set("lamp-y", Value::Float(1.05))?;
    let moved = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(moved != thinner);
    assert!(gpu.scene_shadow_passes() > depth_passes);
    assert_eq!(gpu.scene_instance_uploads(), instance_uploads);

    app.set_shadow(false);
    app.set_light_kind("point".into());
    dynamic.set("shadow", Value::Bool(false))?;
    dynamic.set("light-kind", Value::String("point".into()))?;
    let point = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(sample(&point, behind) > sample(&thinner, behind) + 8.0);

    app.set_light_kind("directional".into());
    app.set_lamp_yaw(180.0);
    app.set_samples(1);
    dynamic.set("light-kind", Value::String("directional".into()))?;
    dynamic.set("lamp-yaw", Value::Float(180.0))?;
    dynamic.set("samples", Value::Int(1))?;
    let directional = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    app.set_samples(16);
    dynamic.set("samples", Value::Int(16))?;
    let many = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(
        many.iter()
            .zip(&directional)
            .all(|(&a, &b)| a.abs_diff(b) <= 1)
    );
    app.set_asymmetry(0.6);
    dynamic.set("asymmetry", Value::Float(0.6))?;
    let forward = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(sample(&forward, behind) > sample(&directional, behind) + 10.0);
    Ok(())
}

#[test]
fn world_space_fog_noise_moves_with_offset_and_reuses_the_shadow_atlas() -> TestResult {
    use fog_noise_ui::FogNoiseDemo;
    use xrack_runtime::scene3d::Point3D;

    const NAME: &str = "examples/puzzle3d/fog_noise.xr";
    const VIEW_BYTES: usize = 600 * 356 * 4;
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("FogNoiseDemo")?;
    let app = FogNoiseDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let render = |aot: &mut PreviewScene,
                  interpreter: &mut PreviewScene,
                  gpu: &mut Renderer,
                  other_gpu: &mut Renderer|
     -> TestResult<Vec<u8>> {
        let png = aot.render(gpu)?.to_vec();
        assert!(png.as_slice() == interpreter.render(other_gpu)?);
        Ok(xrack_assets::decode_png(&png)?.rgba[..VIEW_BYTES].to_vec())
    };
    let pixel = |pixels: &[u8], x: usize, y: usize| -> [u8; 4] {
        let offset = (y * 600 + x) * 4;
        std::array::from_fn(|axis| pixels[offset + axis])
    };
    let camera = SceneCamera::new(0.0, 0.0, 10.0, [600.0, 356.0]).ok_or("camera")?;
    let front = camera.project(Point3D::new(-1.95, -0.43, 3.0)).point;
    let front_pixel = |pixels: &[u8]| pixel(pixels, front.x.0 as usize, front.y.0 as usize);
    let mut app = weak.upgrade().ok_or("missing fog noise scene")?;

    app.set_noise(false);
    app.set_graded(false);
    app.set_lit(false);
    for property in ["noise", "graded", "lit"] {
        dynamic.set(property, Value::Bool(false))?;
    }
    let uniform = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    let instance_uploads = gpu.scene_instance_uploads();
    let mesh_uploads = gpu.scene_mesh_uploads();
    let draws = gpu.scene_draw_stats().color_draws;

    app.set_noise(true);
    dynamic.set("noise", Value::Bool(true))?;
    let noisy = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert_eq!(front_pixel(&uniform), front_pixel(&noisy));
    assert_eq!(pixel(&uniform, 0, 0), pixel(&noisy, 0, 0));
    let red = |pixels: &[u8]| {
        (112..174)
            .flat_map(|y| (230..370).map(move |x| pixel(pixels, x, y)[0]))
            .collect::<Vec<_>>()
    };
    let variation = |line: &[u8]| {
        line.iter().copied().max().unwrap_or(0) - line.iter().copied().min().unwrap_or(0)
    };
    assert!(variation(&red(&noisy)) > variation(&red(&uniform)) + 12);
    assert_eq!(gpu.scene_draw_stats().color_draws - draws, draws);
    assert_eq!(gpu.scene_instance_uploads(), instance_uploads);
    assert_eq!(gpu.scene_mesh_uploads(), mesh_uploads);

    app.set_drift(0.41);
    dynamic.set("drift", Value::Float(0.41))?;
    let shifted = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(shifted != noisy);
    app.set_drift(0.41 + 0.7 * 256.0);
    dynamic.set("drift", Value::Float(0.41 + 0.7 * 256.0))?;
    let wrapped = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(
        wrapped
            .iter()
            .zip(&shifted)
            .all(|(&a, &b)| a.abs_diff(b) <= 1)
    );
    app.set_drift(0.41);
    app.set_graded(true);
    app.set_overlap(true);
    dynamic.set("drift", Value::Float(0.41))?;
    dynamic.set("graded", Value::Bool(true))?;
    dynamic.set("overlap", Value::Bool(true))?;
    let layered = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    app.set_reversed(true);
    dynamic.set("reversed", Value::Bool(true))?;
    let reordered = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(
        layered
            .iter()
            .zip(&reordered)
            .all(|(&a, &b)| a.abs_diff(b) <= 1)
    );

    app.set_lit(true);
    dynamic.set("lit", Value::Bool(true))?;
    let lit = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(lit != layered);
    let shadow_passes = gpu.scene_shadow_passes();
    assert!(shadow_passes > 0);
    app.set_drift(0.82);
    dynamic.set("drift", Value::Float(0.82))?;
    let moved_light = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(moved_light != lit);
    assert_eq!(gpu.scene_shadow_passes(), shadow_passes);
    assert_eq!(gpu.scene_instance_uploads(), instance_uploads);
    let before = gpu.scene_draw_stats();
    assert_eq!(
        moved_light,
        render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?
    );
    assert_eq!(
        gpu.scene_draw_stats().viewport_cache_hits,
        before.viewport_cache_hits + 1
    );

    app.set_density_samples(8);
    dynamic.set("density-samples", Value::Int(8))?;
    let coarse = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(coarse != moved_light);
    app.set_density_samples(64);
    dynamic.set("density-samples", Value::Int(64))?;
    let dense = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(dense != coarse);
    assert_eq!(gpu.scene_shadow_passes(), shadow_passes);
    Ok(())
}

#[test]
fn gpu_sprites_scrub_batch_and_share_opaque_depth_in_both_backends() -> TestResult {
    use particles_ui::ParticleDemo;

    const NAME: &str = "examples/puzzle3d/particles.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ParticleDemo")?;
    let app = ParticleDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let new_renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (new_renderer()?, new_renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        time: f64,
        layers: i64,
        emission: f64,
        cover: bool,
        enabled: bool,
        weighted: bool,
        msaa: bool,
        hdr: bool,
    }
    let base = State {
        time: 0.65,
        layers: 3,
        emission: 100.0,
        cover: true,
        enabled: true,
        weighted: true,
        msaa: false,
        hdr: false,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing particle demo")?;
        app.set_time(state.time);
        app.set_layers(state.layers);
        app.set_emission(state.emission);
        app.set_cover(state.cover);
        app.set_emitting(state.enabled);
        app.set_weighted(state.weighted);
        app.set_msaa(state.msaa);
        app.set_hdr(state.hdr);
        for (name, value) in [
            ("time", Value::Float(state.time)),
            ("layers", Value::Int(state.layers)),
            ("emission", Value::Float(state.emission)),
            ("cover", Value::Bool(state.cover)),
            ("emitting", Value::Bool(state.enabled)),
            ("weighted", Value::Bool(state.weighted)),
            ("msaa", Value::Bool(state.msaa)),
            ("hdr", Value::Bool(state.hdr)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let mesh_uploads = gpu.scene_mesh_uploads();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut other_gpu)?);
        let after = gpu.scene_draw_stats();
        let scene = xrack_assets::decode_png(&png)?.rgba[..620 * 330 * 4].to_vec();
        Ok((
            scene,
            after.particle_draws - before.particle_draws,
            after.particle_instances - before.particle_instances,
            gpu.scene_mesh_uploads() - mesh_uploads,
            after.viewport_renders - before.viewport_renders,
        ))
    };

    let count_orange = |image: &[u8]| {
        image
            .chunks_exact(4)
            .filter(|rgba| rgba[0] > rgba[2].saturating_add(35))
            .count()
    };
    let count_cyan = |image: &[u8]| {
        image
            .chunks_exact(4)
            .filter(|rgba| rgba[1] > rgba[0].saturating_add(35))
            .count()
    };
    let (first, draws, instances, _, _) = render(base)?;
    assert_eq!((draws, instances), (1, 125));
    let (unchanged, draws, _, _, _) = render(base)?;
    assert_eq!(first, unchanged);
    assert_eq!(draws, 0, "a stationary view reuses its completed texture");

    let (moved, draws, instances, geometry_uploads, _) = render(State { time: 0.91, ..base })?;
    assert_ne!(first, moved);
    assert_eq!((draws, instances, geometry_uploads), (1, 151, 0));
    assert_eq!(
        first,
        render(base)?.0,
        "rewinding reconstructs the same sprites"
    );
    assert_eq!((0, 0), {
        let (_, draws, instances, _, _) = render(State { time: 0.0, ..base })?;
        (draws, instances)
    });
    let (expired, _, instances, _, _) = render(State {
        time: 2.0,
        emission: 0.0,
        ..base
    })?;
    assert_eq!(instances, 0, "burst expires at the end of its lifetime");
    assert_eq!(
        expired,
        render(State {
            enabled: false,
            ..base
        })?
        .0
    );
    let (disabled, _, _, _, viewport_renders) = render(State {
        enabled: false,
        time: 30.0,
        ..base
    })?;
    assert_eq!(expired, disabled);
    assert_eq!(
        viewport_renders, 0,
        "the clock is irrelevant with no emitters"
    );

    let (open, draws, _, _, _) = render(State {
        cover: false,
        ..base
    })?;
    assert_eq!(
        draws, 1,
        "a particle-only view records its own depth and color"
    );
    assert!(
        count_orange(&first) + 50 < count_orange(&open),
        "opaque cover tests sprite depth"
    );
    assert!(count_cyan(&open) > 100);
    let (cyan, draws, instances, _, _) = render(State { layers: 2, ..base })?;
    assert_eq!((draws, instances), (1, 60));
    assert_eq!(count_orange(&cyan), 0);
    assert_eq!(count_cyan(&open), count_cyan(&cyan));

    let (hdr, draws, instances, _, _) = render(State {
        msaa: true,
        hdr: true,
        ..base
    })?;
    assert_eq!((draws, instances), (1, 125));
    assert_ne!(hdr, first);
    let (source_over, draws, instances, _, _) = render(State {
        weighted: false,
        msaa: true,
        ..base
    })?;
    assert_eq!((draws, instances), (1, 125));
    assert!(count_cyan(&source_over) > 100);
    let (_, draws, instances, _, _) = render(State {
        emission: 10000.0,
        ..base
    })?;
    assert_eq!((draws, instances), (1, 4096 + 60));

    let invalid = source
        .replace("velocity-spread: vector3d", "velocity-spread: string")
        .replace("Vector3D.new(1.3, 1.6, 0.8)", r#""invalid""#)
        .replace("Vector3D.new(3, 2.4, 1)", r#""invalid""#);
    let diagnostic = match xrack_compiler::prepare_preview(&invalid, NAME, Default::default()) {
        Ok(_) => return Err("invalid emitter schema accepted".into()),
        Err(diagnostic) => diagnostic,
    };
    assert!(
        diagnostic
            .to_string()
            .contains("requires a vector3d field named `velocity-spread`")
    );
    Ok(())
}

#[test]
fn sprite_sheets_animate_share_textures_and_preserve_both_backends() -> TestResult {
    use sprite_particles_ui::SpriteParticlesDemo;

    const NAME: &str = "examples/puzzle3d/sprite_particles.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("SpriteParticlesDemo")?;
    let app = SpriteParticlesDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut interpreter_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        time: f64,
        animation: &'static str,
        interpolate: bool,
        random: bool,
        weighted: bool,
        cells: i64,
    }
    let base = State {
        time: 0.1,
        animation: "loop",
        interpolate: false,
        random: true,
        weighted: true,
        cells: 4,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing sprite particle demo")?;
        app.set_time(state.time);
        app.set_playback(state.animation.into());
        app.set_interpolate_frames(state.interpolate);
        app.set_random_start(state.random);
        app.set_weighted(state.weighted);
        app.set_cells(state.cells);
        for (name, value) in [
            ("time", Value::Float(state.time)),
            ("playback", Value::String(state.animation.into())),
            ("interpolate-frames", Value::Bool(state.interpolate)),
            ("random-start", Value::Bool(state.random)),
            ("weighted", Value::Bool(state.weighted)),
            ("cells", Value::Int(state.cells)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut interpreter_gpu)?);
        let after = gpu.scene_draw_stats();
        assert_eq!(
            after.particle_texture_uploads,
            interpreter_gpu.scene_draw_stats().particle_texture_uploads
        );
        Ok((
            xrack_assets::decode_png(&png)?,
            after.particle_draws - before.particle_draws,
            after.particle_instances - before.particle_instances,
            after.particle_texture_uploads - before.particle_texture_uploads,
        ))
    };
    let viewport_pixel = |image: &xrack_assets::Raster| scene_pixel(image, [66, 77]);
    let (first, draws, sprites, uploads) = render(base)?;
    let [red, green, blue, _] = viewport_pixel(&first);
    assert!(red > 235 && green < 65 && blue < 80);
    assert_eq!((draws, sprites, uploads), (2, 46, 2));
    assert_eq!(
        render(base)?.1,
        0,
        "the stationary viewport caches its texture"
    );

    let (next, draws, sprites, uploads) = render(State { time: 0.42, ..base })?;
    let [red, green, blue, _] = viewport_pixel(&next);
    assert!(green > 225 && red < 95 && blue < 115);
    assert_eq!((draws, sprites, uploads), (2, 46, 0));
    let repeated = render(State { time: 1.3, ..base })?.0;
    assert_eq!(viewport_pixel(&repeated), viewport_pixel(&render(base)?.0));

    let reversed = render(State {
        animation: "reverse",
        ..base
    })?
    .0;
    let [red, green, blue, _] = viewport_pixel(&reversed);
    assert!(red > 235 && green > 170 && blue < 75);
    let back = render(State {
        time: 0.92,
        animation: "ping-pong",
        ..base
    })?
    .0;
    let [red, green, blue, _] = viewport_pixel(&back);
    assert!(blue > 235 && green > 135 && red < 85);
    let reverse_ping = render(State {
        animation: "ping-pong-reverse",
        ..base
    })?
    .0;
    assert_eq!(viewport_pixel(&reversed), viewport_pixel(&reverse_ping));
    let once = render(State {
        time: 1.7,
        animation: "once",
        ..base
    })?
    .0;
    assert_eq!(viewport_pixel(&reversed), viewport_pixel(&once));
    let held = render(State {
        time: 0.94,
        animation: "single-frame",
        ..base
    })?
    .0;
    assert_eq!(viewport_pixel(&repeated), viewport_pixel(&held));

    let mixed = render(State {
        time: 0.15,
        interpolate: true,
        ..base
    })?
    .0;
    let [red, green, blue, _] = viewport_pixel(&mixed);
    assert!(red > 145 && green > 145 && blue < 110, "linear frame blend");
    let (_, draws, sprites, uploads) = render(State {
        weighted: false,
        ..base
    })?;
    assert_eq!(
        (draws, sprites, uploads),
        (3, 46, 0),
        "source-over keeps A/B/A order"
    );
    let still = render(State {
        random: false,
        ..base
    })?
    .0;
    let randomized = render(base)?.0;
    assert_eq!(viewport_pixel(&still), viewport_pixel(&randomized));
    assert_ne!(
        still.rgba[..620 * 320 * 4],
        randomized.rgba[..620 * 320 * 4]
    );

    let (fallback, draws, sprites, uploads) = render(State { cells: 5, ..base })?;
    let [red, green, blue, _] = viewport_pixel(&fallback);
    assert!(
        red > 235 && green > 235 && blue > 235,
        "invalid sheet uses the soft disc"
    );
    assert_eq!((draws, sprites, uploads), (2, 46, 0));
    Ok(())
}

#[test]
fn particle_materials_mix_in_linear_color_and_age_without_reuploads() -> TestResult {
    use particle_materials_ui::ParticleMaterialsDemo;

    const NAME: &str = "examples/puzzle3d/particle_materials.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ParticleMaterialsDemo")?;
    let app = ParticleMaterialsDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut interpreter_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        time: f64,
        blend: &'static str,
        table: bool,
        sprite: bool,
        companion: bool,
        cover: bool,
        weighted: bool,
        msaa: bool,
        hdr: bool,
    }
    let base = State {
        time: 0.41,
        blend: "source-over",
        table: false,
        sprite: false,
        companion: false,
        cover: false,
        weighted: true,
        msaa: false,
        hdr: false,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing particle materials demo")?;
        app.set_time(state.time);
        app.set_blend_mode(state.blend.into());
        app.set_use_table(state.table);
        app.set_use_sprite(state.sprite);
        app.set_companion(state.companion);
        app.set_cover(state.cover);
        app.set_weighted(state.weighted);
        app.set_msaa(state.msaa);
        app.set_hdr(state.hdr);
        for (name, value) in [
            ("time", Value::Float(state.time)),
            ("blend-mode", Value::String(state.blend.into())),
            ("use-table", Value::Bool(state.table)),
            ("use-sprite", Value::Bool(state.sprite)),
            ("companion", Value::Bool(state.companion)),
            ("cover", Value::Bool(state.cover)),
            ("weighted", Value::Bool(state.weighted)),
            ("msaa", Value::Bool(state.msaa)),
            ("hdr", Value::Bool(state.hdr)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut interpreter_gpu)?);
        let after = gpu.scene_draw_stats();
        assert_eq!(
            after.particle_texture_uploads,
            interpreter_gpu.scene_draw_stats().particle_texture_uploads
        );
        Ok((
            xrack_assets::decode_png(&png)?,
            after.particle_draws - before.particle_draws,
            after.particle_texture_uploads - before.particle_texture_uploads,
            after.transparency_composites - before.transparency_composites,
        ))
    };
    let viewport_pixel = |image: &xrack_assets::Raster| scene_pixel(image, [68, 76]);
    let linear = |number: u8| {
        let number = f64::from(number) / 255.0;
        ((number + 0.055) / 1.055).powf(2.4)
    };
    let srgb = |number: f64| ((1.055 * number.powf(1.0 / 2.4) - 0.055) * 255.0).round() as u8;
    let expected = |blend| -> [u8; 3] {
        std::array::from_fn(|i| {
            let source = linear([192, 96, 48][i]) * 128.0 / 255.0;
            let dst = linear([96, 112, 128][i]);
            let combined = match blend {
                "additive" => source + dst,
                "screen" => source + dst * (1.0 - source),
                "multiply" => dst * (1.0 - 128.0 / 255.0 + source),
                _ => source + dst * (1.0 - 128.0 / 255.0),
            };
            srgb(combined)
        })
    };
    for mode in ["source-over", "additive", "screen", "multiply"] {
        let (image, draws, uploads, resolves) = render(State {
            blend: mode,
            ..base
        })?;
        let pixel = viewport_pixel(&image);
        for (actual, expected) in pixel[..3].iter().zip(expected(mode)) {
            assert!(actual.abs_diff(expected) <= 3, "{mode} produces {pixel:?}");
        }
        assert_eq!(
            (draws, uploads, resolves),
            (1, 0, u64::from(mode == "source-over"))
        );
    }
    let (mixed, draws, _, resolves) = render(State {
        blend: "screen",
        companion: true,
        msaa: true,
        ..base
    })?;
    let [r, g, b, _] = viewport_pixel(&mixed);
    assert_eq!(
        (draws, resolves),
        (2, 1),
        "screen and weighted particles share opaque depth"
    );
    assert!(
        [r, g, b]
            .iter()
            .zip(expected("screen"))
            .all(|(a, b)| a.abs_diff(b) <= 3)
    );
    let hdr = render(State {
        blend: "screen",
        companion: true,
        msaa: true,
        hdr: true,
        ..base
    })?
    .0;
    assert_ne!(hdr.rgba, mixed.rgba);

    let ramp = State {
        time: 0.0,
        table: true,
        weighted: false,
        ..base
    };
    let (young, _, uploads, _) = render(ramp)?;
    let opening = viewport_pixel(&young);
    assert_eq!(uploads, 1);
    assert!(opening[0] > opening[1] + 35);
    let (grown, _, uploads, _) = render(State { time: 2.0, ..ramp })?;
    assert_eq!(uploads, 0, "changing age retains the color table");
    assert_ne!(opening, viewport_pixel(&grown));
    let faded = viewport_pixel(&render(State { time: 3.99, ..ramp })?.0);
    let background = [96, 112, 128];
    let contrast = |pixel: [u8; 4]| -> u16 {
        pixel[..3]
            .iter()
            .zip(background)
            .map(|(channel, backdrop)| u16::from(channel.abs_diff(backdrop)))
            .sum()
    };
    assert!(contrast(faded) < contrast(opening) / 2);
    assert_eq!(render(ramp)?.0.rgba, young.rgba);

    let ring = render(State {
        sprite: true,
        table: true,
        blend: "multiply",
        ..base
    })?
    .0;
    assert_eq!(&viewport_pixel(&ring)[..3], &background);
    let covered = State {
        cover: true,
        blend: "additive",
        ..base
    };
    assert_eq!(
        viewport_pixel(&render(covered)?.0),
        viewport_pixel(
            &render(State {
                blend: "multiply",
                ..covered
            })?
            .0
        ),
        "an opaque object hides special-blend particles"
    );
    Ok(())
}

#[test]
fn particle_emission_geometry_and_directions_replay_in_both_backends() -> TestResult {
    use particle_emitters_ui::ParticleEmittersDemo;

    const NAME: &str = "examples/puzzle3d/particle_emitters.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ParticleEmittersDemo")?;
    let app = ParticleEmittersDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut dynamic_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        shape: &'static str,
        direction: &'static str,
        filled: bool,
        angle: f64,
        turn: f64,
        time: f64,
    }
    let base = State {
        shape: "box",
        direction: "cone",
        filled: true,
        angle: 28.0,
        turn: 0.0,
        time: 0.62,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing spatial particle demo")?;
        app.set_shape(state.shape.into());
        app.set_direction(state.direction.into());
        app.set_filled(state.filled);
        app.set_angle(state.angle);
        app.set_turn(state.turn);
        app.set_time(state.time);
        for (name, value) in [
            ("shape", Value::String(state.shape.into())),
            ("direction", Value::String(state.direction.into())),
            ("filled", Value::Bool(state.filled)),
            ("angle", Value::Float(state.angle)),
            ("turn", Value::Float(state.turn)),
            ("time", Value::Float(state.time)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut dynamic_gpu)?);
        let after = gpu.scene_draw_stats();
        Ok((
            xrack_assets::decode_png(&png)?,
            after.particle_draws - before.particle_draws,
            after.particle_instances - before.particle_instances,
        ))
    };
    let blue = |pixel: &[u8; 4]| pixel[2] > 185 && pixel[1] > 120 && pixel[0] < 110;
    let orange = |pixel: &[u8; 4]| pixel[0] > 185 && pixel[1] > 70 && pixel[2] < 112;
    let footprint = |image: &xrack_assets::Raster, color: fn(&[u8; 4]) -> bool| {
        let [width, _] = image.size.map(|value| value as usize);
        let mut bounds = [width, 360, 0, 0];
        for (index, pixel) in image.rgba[..width * 360 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            if color(pixel) {
                let [x, y] = [index % width, index / width];
                bounds = [
                    bounds[0].min(x),
                    bounds[1].min(y),
                    bounds[2].max(x),
                    bounds[3].max(y),
                ];
            }
        }
        assert!(bounds[0] <= bounds[2] && bounds[1] <= bounds[3]);
        bounds
    };
    let corner = |image: &xrack_assets::Raster| {
        let width = image.size[0] as usize;
        (133..141)
            .flat_map(|y| (115..129).map(move |x| x + y * width))
            .filter(|&index| blue(&image.rgba.as_chunks::<4>().0[index]))
            .count()
    };
    let (boxed, draws, instances) = render(base)?;
    assert_eq!(
        (draws, instances),
        (1, 1042),
        "ordinary and spatial emitters stay batched"
    );
    assert_eq!(render(base)?.1, 0);
    let boxed_bounds = footprint(&boxed, blue);
    assert!(boxed_bounds[2] - boxed_bounds[0] >= 142 && corner(&boxed) > 5);

    let rotated = render(State { turn: 90.0, ..base })?.0;
    let turned = footprint(&rotated, blue);
    assert!(turned[2] - turned[0] < 112 && turned[3] - turned[1] > 143);
    let volume = render(State {
        shape: "sphere",
        ..base
    })?
    .0;
    assert_eq!(corner(&volume), 0);
    let shell = render(State {
        shape: "sphere",
        filled: false,
        ..base
    })?
    .0;
    assert_ne!(volume.rgba, shell.rgba);
    assert_eq!(corner(&shell), 0);
    let cylinder = render(State {
        shape: "cylinder",
        filled: false,
        ..base
    })?
    .0;
    assert_ne!(cylinder.rgba, shell.rgba);
    assert_eq!(
        render(base)?.0.rgba,
        boxed.rgba,
        "rewinding restores the same samples"
    );

    let narrow = render(State { angle: 0.0, ..base })?.0;
    let wide = render(State {
        angle: 55.0,
        ..base
    })?
    .0;
    let narrow = footprint(&narrow, orange);
    let wide = footprint(&wide, orange);
    assert!(wide[2] - wide[0] > 2 * (narrow[2] - narrow[0]));
    let target = State {
        direction: "target",
        ..base
    };
    let start = footprint(
        &render(State {
            time: 0.0,
            ..target
        })?
        .0,
        orange,
    );
    let near = footprint(
        &render(State {
            time: 0.8,
            ..target
        })?
        .0,
        orange,
    );
    assert!(start[2] - start[0] > 50 && near[2] - near[0] < 24);
    let fixed_speed = render(State {
        direction: "normalized-vector",
        ..base
    })?
    .0;
    let varied_speed = render(State {
        direction: "vector",
        ..base
    })?
    .0;
    assert_ne!(fixed_speed.rgba, varied_speed.rgba);

    let points = render(State {
        shape: "point",
        ..base
    })?
    .0;
    let small = footprint(&points, blue);
    assert!(small[2] - small[0] < 7 && small[3] - small[1] < 7);
    // The normal point emitters before and after the spatial rows keep their
    // positions when the optional motion buffer is enabled for the batch.
    let sprites = boxed.rgba.as_chunks::<4>().0;
    for (x, y) in [(314, 222), (467, 111)] {
        let [r, g, b, _] = sprites[x + y * boxed.size[0] as usize];
        assert!(r > 135 && g > 150 && b > 175);
    }
    Ok(())
}

#[test]
fn mesh_particle_emitters_match_backends_and_reuse_surface_tables() -> TestResult {
    use particle_mesh_emitters_ui::ParticleMeshEmittersDemo;

    const NAME: &str = "examples/puzzle3d/particle_mesh_emitters.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ParticleMeshEmittersDemo")?;
    let app = ParticleMeshEmittersDemo::new();
    let original_key = app.get_mesh_key();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut dynamic_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        mesh: Option<&'static str>,
        direction: &'static str,
        stretch: f64,
        turn: f64,
        time: f64,
    }
    let base = State {
        mesh: None,
        direction: "surface-normal",
        stretch: 1.0,
        turn: 0.0,
        time: 0.0,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mesh = state.mesh.unwrap_or(&original_key);
        let mut app = weak.upgrade().ok_or("missing model surface demo")?;
        app.set_mesh_key(mesh.into());
        app.set_direction(state.direction.into());
        app.set_stretch(state.stretch);
        app.set_turn(state.turn);
        app.set_time(state.time);
        for (name, value) in [
            ("mesh-key", Value::String(mesh.into())),
            ("direction", Value::String(state.direction.into())),
            ("stretch", Value::Float(state.stretch)),
            ("turn", Value::Float(state.turn)),
            ("time", Value::Float(state.time)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut dynamic_gpu)?);
        let after = gpu.scene_draw_stats();
        assert_eq!(
            after.particle_mesh_uploads,
            dynamic_gpu.scene_draw_stats().particle_mesh_uploads
        );
        Ok((
            xrack_assets::decode_png(&png)?,
            after.particle_draws - before.particle_draws,
            after.particle_instances - before.particle_instances,
            after.particle_mesh_uploads - before.particle_mesh_uploads,
        ))
    };
    let blue = |p: &[u8; 4]| p[2] > 185 && p[1] > 120 && p[0] < 110;
    let orange = |p: &[u8; 4]| p[0] > 185 && p[1] > 70 && p[2] < 112;
    let footprints = |image: &xrack_assets::Raster, color: fn(&[u8; 4]) -> bool| {
        let width = image.size[0] as usize;
        let mut sides = [(0, 0), (0, 0)];
        for (i, p) in image.rgba[..width * 352 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            if color(p) {
                let x = i % width;
                let side = &mut sides[usize::from(x >= width / 2)];
                side.0 += x;
                side.1 += 1;
            }
        }
        sides
    };

    let (birth, draws, instances, uploads) = render(base)?;
    assert_eq!((draws, instances, uploads), (1, 1600, 1));
    assert_eq!(render(base)?.1, 0);
    let blue_points = footprints(&birth, blue);
    assert!(blue_points[0].1 > 200 && blue_points[1].1 > 60);
    assert!(
        blue_points[0].1 > blue_points[1].1 * 3,
        "the larger triangle receives more particles"
    );
    let orange_birth = footprints(&birth, orange);
    let (moving, draws, instances, uploads) = render(State { time: 0.55, ..base })?;
    assert_eq!((draws, instances, uploads), (1, 1600, 0));
    assert_eq!(footprints(&moving, blue), blue_points);
    let orange_moving = footprints(&moving, orange);
    let center = |sum_count: (usize, usize)| sum_count.0 / sum_count.1.max(1);
    assert!(center(orange_moving[0]) + 25 < center(orange_birth[0]));
    assert!(center(orange_moving[1]) > center(orange_birth[1]) + 25);
    let aiming = render(State {
        time: 0.55,
        direction: "target",
        ..base
    })?
    .0;
    let toward_center = footprints(&aiming, orange);
    assert!(center(toward_center[0]) > center(orange_birth[0]) + 25);
    assert!(center(toward_center[1]) + 25 < center(orange_birth[1]));

    let (rotated, _, _, uploads) = render(State { turn: 18.0, ..base })?;
    assert_ne!(rotated.rgba, birth.rgba);
    assert_eq!(
        uploads, 0,
        "rotation reuses the scale-weighted surface table"
    );
    let (scaled, _, _, uploads) = render(State {
        stretch: 1.8,
        ..base
    })?;
    assert_ne!(scaled.rgba, birth.rgba);
    assert_eq!(
        uploads, 1,
        "different scale builds one new table for both emitters"
    );
    let (replay, _, _, uploads) = render(base)?;
    assert_eq!(replay.rgba, birth.rgba);
    assert_eq!(uploads, 0, "cached scale is recovered without an upload");
    let (_, draws, instances, uploads) = render(State {
        mesh: Some("primitive:box"),
        ..base
    })?;
    assert_eq!((draws, instances, uploads), (1, 1600, 1));
    let (_, draws, instances, uploads) = render(State {
        mesh: Some("missing"),
        ..base
    })?;
    assert_eq!(
        (draws, instances, uploads),
        (0, 0, 0),
        "an unresolved mesh has no emission surface"
    );
    Ok(())
}

#[test]
fn particle_affectors_follow_age_layers_and_row_order_in_both_backends() -> TestResult {
    use particle_affectors_ui::ParticleAffectorsDemo;

    const NAME: &str = "examples/puzzle3d/particle_affectors.xr";
    let text = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&text, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ParticleAffectorsDemo")?;
    let app = ParticleAffectorsDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        time: f64,
        kind: &'static str,
        enabled: bool,
        hide: bool,
        secondary: bool,
        reverse: bool,
        layers: i64,
        visible: i64,
        variation: f64,
    }
    let base = State {
        time: 0.58,
        kind: "attractor",
        enabled: true,
        hide: false,
        secondary: false,
        reverse: false,
        layers: 1,
        visible: 3,
        variation: 0.0,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing particle affectors demo")?;
        app.set_time(state.time);
        app.set_field_kind(state.kind.into());
        app.set_enabled_fields(state.enabled);
        app.set_hide(state.hide);
        app.set_secondary(state.secondary);
        app.set_reverse_order(state.reverse);
        app.set_layers(state.layers);
        app.set_visible_layers(state.visible);
        app.set_variation(state.variation);
        for (name, value) in [
            ("time", Value::Float(state.time)),
            ("field-kind", Value::String(state.kind.into())),
            ("enabled-fields", Value::Bool(state.enabled)),
            ("hide", Value::Bool(state.hide)),
            ("secondary", Value::Bool(state.secondary)),
            ("reverse-order", Value::Bool(state.reverse)),
            ("layers", Value::Int(state.layers)),
            ("visible-layers", Value::Int(state.visible)),
            ("variation", Value::Float(state.variation)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut other_gpu)?);
        let after = gpu.scene_draw_stats();
        assert_eq!(
            after.particle_affector_uploads,
            other_gpu.scene_draw_stats().particle_affector_uploads
        );
        Ok((
            xrack_assets::decode_png(&png)?,
            after.particle_draws - before.particle_draws,
            after.particle_instances - before.particle_instances,
            after.particle_affector_uploads - before.particle_affector_uploads,
        ))
    };
    let blue = |pixel: &[u8; 4]| pixel[2] > 185 && pixel[1] > 120 && pixel[0] < 120;
    let orange = |pixel: &[u8; 4]| pixel[0] > 185 && pixel[1] > 70 && pixel[2] < 112;
    let measure = |image: &xrack_assets::Raster, color: fn(&[u8; 4]) -> bool| {
        let width = image.size[0] as usize;
        let mut sum = [0, 0, 0];
        for (i, p) in image.rgba[..width * 352 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            if color(p) {
                sum[0] += 1;
                sum[1] += i % width;
                sum[2] += i / width;
            }
        }
        [sum[0], sum[1] / sum[0].max(1), sum[2] / sum[0].max(1)]
    };

    let early = State { time: 0.1, ..base };
    let (early_image, _, _, uploads) = render(early)?;
    assert_eq!(
        uploads, 0,
        "fields which have not started use the plain particle pipeline"
    );
    let early_disabled = render(State {
        enabled: false,
        ..early
    })?
    .0;
    assert_eq!(
        early_image.rgba[..670 * 352 * 4],
        early_disabled.rgba[..670 * 352 * 4]
    );
    let unaffected = render(State {
        enabled: false,
        ..base
    })?
    .0;
    let (attracted, draws, instances, uploads) = render(base)?;
    assert_eq!((draws, instances, uploads), (1, 960, 1));
    assert!(measure(&attracted, blue)[1] > measure(&unaffected, blue)[1] + 30);
    assert_eq!(measure(&attracted, orange), measure(&unaffected, orange));
    assert_eq!(
        render(State { time: 0.72, ..base })?.3,
        0,
        "time alone retains field uniforms"
    );
    assert_eq!(
        render(base)?.0.rgba,
        attracted.rgba,
        "rewinding restores the same position"
    );
    let (other_layers, _, _, uploads) = render(State { layers: 4, ..base })?;
    assert_eq!(uploads, 0);
    assert_eq!(
        other_layers.rgba[..670 * 352 * 4],
        unaffected.rgba[..670 * 352 * 4]
    );
    let (visible, _, instances, uploads) = render(State { visible: 2, ..base })?;
    assert_eq!((instances, uploads), (480, 0));
    assert_eq!(measure(&visible, blue)[0], 0);

    let arrived = State { time: 1.15, ..base };
    let arrived_image = render(arrived)?.0;
    let center = measure(&arrived_image, blue);
    assert!(center[0] > 0 && center[1].abs_diff(373) < 4 && center[2].abs_diff(135) < 4);
    let hidden = render(State {
        hide: true,
        ..arrived
    })?
    .0;
    assert_eq!(measure(&hidden, blue)[0], 0);
    let ordered = State {
        secondary: true,
        ..arrived
    };
    let then_gravity = render(ordered)?.0;
    let then_attractor = render(State {
        reverse: true,
        ..ordered
    })?
    .0;
    assert!(measure(&then_gravity, blue)[2] > measure(&then_attractor, blue)[2] + 70);
    assert_eq!(measure(&then_attractor, blue), center);

    let later = State { time: 0.9, ..base };
    let original = render(State {
        enabled: false,
        ..later
    })?
    .0;
    let repelled = render(State {
        kind: "repeller",
        ..later
    })?
    .0;
    let gravity = render(State {
        kind: "gravity",
        ..later
    })?
    .0;
    let rotated = render(State {
        kind: "point-rotator",
        ..base
    })?
    .0;
    assert!(measure(&repelled, blue)[1] + 15 < measure(&original, blue)[1]);
    assert!(measure(&gravity, blue)[2] > measure(&original, blue)[2] + 20);
    assert!(measure(&rotated, blue)[2] > measure(&unaffected, blue)[2] + 40);
    let variation = State {
        variation: 0.35,
        ..arrived
    };
    let randomized = render(variation)?.0;
    assert!(measure(&randomized, blue)[0] > center[0] * 10);
    render(base)?;
    assert_eq!(render(variation)?.0.rgba, randomized.rgba);
    Ok(())
}

#[test]
fn particle_trails_follow_curves_rewind_and_fade_after_life_in_both_backends() -> TestResult {
    use particle_trails_ui::ParticleTrailsDemo;

    const NAME: &str = "examples/puzzle3d/particle_trails.xr";
    let text = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&text, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ParticleTrailsDemo")?;
    let app = ParticleTrailsDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        time: f64,
        segments: i64,
        history: f64,
        rotate: bool,
        retain: bool,
    }
    let base = State {
        time: 0.9,
        segments: 12,
        history: 0.8,
        rotate: false,
        retain: true,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing particle trails demo")?;
        app.set_time(state.time);
        app.set_segments(state.segments);
        app.set_history(state.history);
        app.set_rotate_trails(state.rotate);
        app.set_retain_trails(state.retain);
        for (name, value) in [
            ("time", Value::Float(state.time)),
            ("segments", Value::Int(state.segments)),
            ("history", Value::Float(state.history)),
            ("rotate-trails", Value::Bool(state.rotate)),
            ("retain-trails", Value::Bool(state.retain)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut other_gpu)?);
        let after = gpu.scene_draw_stats();
        Ok((
            xrack_assets::decode_png(&png)?,
            after.particle_draws - before.particle_draws,
            after.particle_instances - before.particle_instances,
            after.particle_trail_segments - before.particle_trail_segments,
        ))
    };
    let blue = |pixel: &[u8; 4]| pixel[2] > 155 && pixel[1] > 105 && pixel[0] < 120;
    let bounds = |image: &xrack_assets::Raster| {
        let width = image.size[0] as usize;
        let mut min = width;
        let mut max = 0;
        for (i, pixel) in image.rgba[..width * 352 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            if blue(pixel) {
                let x = i % width;
                min = min.min(x);
                max = max.max(x);
            }
        }
        (min <= max).then_some((min, max))
    };
    let y_midpath = |image: &xrack_assets::Raster| {
        let width = image.size[0] as usize;
        let mut sum = 0;
        let mut count = 0;
        for y in 0..352 {
            for x in 308..327 {
                if blue(&image.rgba.as_chunks::<4>().0[x + y * width]) {
                    sum += y;
                    count += 1;
                }
            }
        }
        assert!(count > 10);
        sum / count
    };

    let (curve, draws, instances, segments) = render(base)?;
    assert_eq!((draws, instances, segments), (2, 2, 12));
    assert_eq!(render(base)?.1, 0);
    let ((left, right), (short_left, short_right)) = (
        bounds(&curve).ok_or("missing curve")?,
        bounds(
            &render(State {
                history: 0.35,
                ..base
            })?
            .0,
        )
        .ok_or("missing short trail")?,
    );
    assert!(right - left > 130 && short_left > left + 55 && right.abs_diff(short_right) < 3);
    let (chord, _, _, segments) = render(State {
        segments: 1,
        ..base
    })?;
    assert_eq!(segments, 1);
    assert!(y_midpath(&curve) + 15 < y_midpath(&chord));
    let (points, draws, _, segments) = render(State {
        segments: 0,
        ..base
    })?;
    assert_eq!((draws, segments), (1, 0));
    let (point_min, point_max) = bounds(&points).ok_or("missing point sprite")?;
    assert!(point_max - point_min < 17);

    let rotated = render(State {
        rotate: true,
        ..base
    })?
    .0;
    assert_ne!(rotated.rgba[..670 * 352 * 4], curve.rgba[..670 * 352 * 4]);
    assert_eq!(render(base)?.0.rgba, curve.rgba);
    let early = render(State { time: 0.0, ..base })?.0;
    assert!(
        bounds(&early).is_none(),
        "a newborn particle has no prior trail"
    );

    let end = render(State { time: 1.25, ..base })?.0;
    let (faded, _, instances, segments) = render(State { time: 1.45, ..base })?;
    assert_eq!((instances, segments), (2, 12));
    let (_, end_x) = bounds(&end).ok_or("missing trail at end of life")?;
    let (_, faded_x) = bounds(&faded).ok_or("missing retained trail")?;
    assert!(
        end_x.abs_diff(faded_x) <= 2,
        "retained trail holds its terminal geometry"
    );
    let energy = |image: &xrack_assets::Raster| -> u32 {
        image.rgba[..670 * 352 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| u32::from(p[2].saturating_sub(100)))
            .sum()
    };
    assert!(energy(&faded) < energy(&end));
    assert!(
        bounds(
            &render(State {
                time: 1.45,
                retain: false,
                ..base
            })?
            .0
        )
        .is_none()
    );
    let (expired, _, instances, segments) = render(State { time: 1.96, ..base })?;
    assert_eq!((instances, segments), (1, 0));
    assert!(bounds(&expired).is_none());
    Ok(())
}

#[test]
fn builtin_shapes_share_picking_sweeps_slots_particles_and_gpu_buffers() -> TestResult {
    use primitive_meshes_ui::PrimitiveMeshesDemo;
    use xrack_runtime::Hit3D;

    const NAME: &str = "examples/puzzle3d/primitive_meshes.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("PrimitiveMeshesDemo")?;
    let app = PrimitiveMeshesDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other) = (renderer()?, renderer()?);
    let check_hit = |name, hit: Hit3D, id: &str| -> TestResult<Hit3D> {
        assert!(hit.valid, "{name}");
        assert_eq!(hit.object_id, id, "{name}");
        assert_eq!(dynamic.get(name)?, Value::Hit3D(hit.clone()), "{name}");
        Ok(hit)
    };
    let near = |actual: f64, expected: f64| {
        assert!((actual - expected).abs() < 1e-3, "{actual} != {expected}");
    };
    {
        let app = weak.upgrade().ok_or("missing primitives demo")?;
        let rectangle = check_hit("rectangle-hit", app.get_rectangle_hit(), "rectangle")?;
        near(rectangle.position.z, 0.30);
        near(rectangle.u, 0.5);
        near(rectangle.v, 0.5);
        let side = check_hit("cylinder-hit", app.get_cylinder_hit(), "cylinder")?;
        near(side.position.z, 0.98);
        let cap = check_hit("cap-hit", app.get_cap_hit(), "cylinder")?;
        near(cap.position.y, 1.27);
        near(cap.normal.y, 1.0);
        near(cap.u, 0.5);
        near(cap.v, 0.5);
        let cone = check_hit("cone-hit", app.get_cone_hit(), "cone")?;
        near(cone.position.z, 0.22 + 0.98 * (1.0 - 1.14 / 2.43));
        let sweep = check_hit("sweep", app.get_sweep(), "cylinder")?;
        near(sweep.distance, 0.13);
        for (name, shape, hit) in [
            ("inside-cylinder", "cylinder", app.get_inside_cylinder()),
            ("inside-cone", "cone", app.get_inside_cone()),
        ] {
            near(check_hit(name, hit, shape)?.distance, 0.0);
        }
        near(
            check_hit("sweep-rectangle", app.get_sweep_rectangle(), "rectangle")?.distance,
            1.0,
        );
    }
    let mirrored = |aot: &mut PreviewScene,
                    interpreted: &mut PreviewScene,
                    gpu: &mut Renderer,
                    other: &mut Renderer|
     -> TestResult<Vec<u8>> {
        let png = aot.render(gpu)?.to_vec();
        assert_eq!(png, interpreted.render(other)?);
        Ok(xrack_assets::decode_png(&png)?.rgba[..650 * 328 * 4].to_vec())
    };
    let first = mirrored(&mut aot, &mut interpreted, &mut gpu, &mut other)?;
    let mesh_uploads = gpu.scene_mesh_uploads();
    let instance_uploads = gpu.scene_instance_uploads();
    let shadow_passes = gpu.scene_shadow_passes();
    weak.upgrade()
        .ok_or("missing primitives demo")?
        .set_warm_cap(false);
    dynamic.set("warm-cap", Value::Bool(false))?;
    let recolored = mirrored(&mut aot, &mut interpreted, &mut gpu, &mut other)?;
    assert_ne!(first, recolored);
    assert_eq!(
        [
            gpu.scene_mesh_uploads(),
            gpu.scene_instance_uploads(),
            gpu.scene_shadow_passes()
        ],
        [mesh_uploads, instance_uploads, shadow_passes]
    );

    let mut app = weak.upgrade().ok_or("missing primitives demo")?;
    app.set_cutout(true);
    dynamic.set("cutout", Value::Bool(true))?;
    check_hit("rectangle-hit", app.get_rectangle_hit(), "wall")?;
    near(
        check_hit("sweep-rectangle", app.get_sweep_rectangle(), "rectangle")?.distance,
        1.0,
    );
    let masked = mirrored(&mut aot, &mut interpreted, &mut gpu, &mut other)?;
    assert_ne!(masked, recolored);

    let before = gpu.scene_draw_stats();
    app.set_particles(true);
    dynamic.set("particles", Value::Bool(true))?;
    let particles = mirrored(&mut aot, &mut interpreted, &mut gpu, &mut other)?;
    assert_ne!(particles, masked);
    let after = gpu.scene_draw_stats();
    assert_eq!(
        after.particle_model_instances - before.particle_model_instances,
        24
    );
    assert!(after.particle_model_draws > before.particle_model_draws);
    assert!(after.particle_mesh_uploads > before.particle_mesh_uploads);
    assert_eq!(gpu.scene_mesh_uploads(), mesh_uploads);
    Ok(())
}

#[test]
fn source_material_slots_isolate_color_and_picking_in_both_backends() -> TestResult {
    use material_slots_ui::MaterialSlotsDemo;

    const NAME: &str = "examples/puzzle3d/material_slots.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialSlotsDemo")?;
    let app = MaterialSlotsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    struct Frame {
        patches: [Vec<[u8; 4]>; 3],
        picked: String,
        uploads: [u64; 3],
    }
    let mut render = |target: &str, warm, cutout, accent, implicit| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing material slot demo")?;
        app.set_target(target.into());
        app.set_warm(warm);
        app.set_cutout(cutout);
        app.set_accent(accent);
        app.set_implicit_style(implicit);
        for (name, value) in [
            ("target", Value::String(target.into())),
            ("warm", Value::Bool(warm)),
            ("cutout", Value::Bool(cutout)),
            ("accent", Value::Bool(accent)),
            ("implicit-style", Value::Bool(implicit)),
        ] {
            dynamic.set(name, value)?;
        }
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png, interpreted.render(&mut preview_gpu)?);
        let picked = app.get_body_hit();
        assert_eq!(dynamic.get("body-hit")?, Value::String(picked.clone()));
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing slot scene".into());
        };
        let surface = scene
            .iter()
            .find(|object| object.id == "model")
            .ok_or("missing glTF surface")?;
        let slots = surface.mesh.material_slots();
        assert_eq!(
            slots
                .iter()
                .map(|slot| slot.source_index)
                .collect::<Vec<_>>(),
            [Some(2), Some(0), None]
        );
        assert_eq!(slots[0].name.as_deref(), Some("Body finish"));
        assert_eq!(slots[1].name.as_deref(), Some("Trim"));

        let raster = xrack_assets::decode_png(&png)?;
        let pixels = raster.rgba.as_chunks::<4>().0;
        let patches = [183, 317, 451].map(|x| {
            (145..179)
                .flat_map(|y| pixels[y * 634 + x - 17..y * 634 + x + 17].iter().copied())
                .collect()
        });
        Ok(Frame {
            patches,
            picked,
            uploads: [
                gpu.scene_mesh_uploads(),
                gpu.scene_instance_uploads(),
                gpu.scene_shadow_passes(),
            ],
        })
    };

    let warm = render("Body finish", true, false, true, true)?;
    let cool = render("Body finish", false, false, true, true)?;
    assert_ne!(warm.patches[0], cool.patches[0]);
    assert_eq!(warm.patches[1..], cool.patches[1..]);
    assert_eq!(warm.uploads, cool.uploads);
    assert_eq!(cool.picked, "model");

    let by_index = render("#2", false, false, true, true)?;
    assert_eq!(cool.patches, by_index.patches);
    assert_eq!(cool.uploads, by_index.uploads);
    let unused = render("#1", false, false, true, true)?;
    assert_ne!(unused.patches[0], by_index.patches[0]);
    assert_eq!(unused.patches[1..], by_index.patches[1..]);

    let trim = render("#2", false, false, false, true)?;
    assert_ne!(trim.patches[1], by_index.patches[1]);
    assert_eq!(trim.patches[0], by_index.patches[0]);
    assert_eq!(trim.patches[2], by_index.patches[2]);
    let unstyled_default = render("#2", false, false, true, false)?;
    assert_ne!(unstyled_default.patches[2], by_index.patches[2]);
    assert_eq!(unstyled_default.patches[..2], by_index.patches[..2]);

    let mask = render("#2", false, true, true, true)?;
    assert_eq!(mask.picked, "wall");
    assert_ne!(mask.patches[0], by_index.patches[0]);
    assert_eq!(mask.patches[1..], by_index.patches[1..]);
    assert_eq!(mask.uploads[..2], by_index.uploads[..2]);
    assert!(mask.uploads[2] > by_index.uploads[2]);
    Ok(())
}

#[test]
fn authored_textures_share_mips_geometry_and_alpha_picking_in_both_backends() -> TestResult {
    use authored_texture_ui::AuthoredTextureDemo;
    use xrack_runtime::scene3d::TextureColorSpace;

    const NAME: &str = "examples/puzzle3d/authored_texture.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("AuthoredTextureDemo")?;
    let app = AuthoredTextureDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    struct Frame {
        pixels: xrack_assets::Raster,
        picked: String,
        gpu: [u64; 4],
    }
    let mut draw = |repeat, rotation, bump, cutout, imported, wrap: &str| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing authored texture demo")?;
        app.set_repeat(repeat);
        app.set_rotation(rotation);
        app.set_bump(bump);
        app.set_cutout(cutout);
        app.set_imported(imported);
        app.set_wrap(wrap.into());
        for (name, value) in [
            ("repeat", Value::Float(repeat)),
            ("rotation", Value::Float(rotation)),
            ("bump", Value::Bool(bump)),
            ("cutout", Value::Bool(cutout)),
            ("imported", Value::Bool(imported)),
            ("wrap", Value::String(wrap.into())),
        ] {
            dynamic.set(name, value)?;
        }
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png, interpreted.render(&mut preview_gpu)?);
        let picked = app.get_picked();
        assert_eq!(dynamic.get("picked")?, Value::String(picked.clone()));
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(scene)) = view.properties.get("model") else {
            return Err("missing texture scene".into());
        };
        let panel = scene
            .iter()
            .find(|object| object.id == "panel")
            .ok_or("missing panel")?;
        let texture = panel.mesh.materials()[0]
            .base_texture
            .as_ref()
            .ok_or("missing color map")?;
        assert!(
            TextureColorSpace::ALL
                .iter()
                .all(|&color| texture.image.prepared_mips(color).is_some())
        );
        Ok(Frame {
            pixels: xrack_assets::decode_png(&png)?,
            picked,
            gpu: [
                gpu.scene_mesh_uploads(),
                gpu.scene_instance_uploads(),
                gpu.scene_draw_stats().material_mip_levels_uploaded,
                gpu.scene_shadow_passes(),
            ],
        })
    };
    let patch = |frame: &Frame, xs: std::ops::Range<usize>| {
        frame
            .pixels
            .rgba
            .chunks_exact(644 * 4)
            .skip(82)
            .take(141)
            .flat_map(|row| row[4 * xs.start..4 * xs.end].iter().copied())
            .collect::<Vec<_>>()
    };
    let initial = draw(1.5, 0.0, true, false, false, "repeat")?;
    assert_eq!(initial.picked, "panel");
    assert!(initial.gpu[2] > 0);
    let tiled = draw(3.8, 33.0, true, false, false, "mirror")?;
    assert_ne!(patch(&initial, 145..285), patch(&tiled, 145..285));
    assert_eq!(
        tiled.gpu, initial.gpu,
        "UV changes reuse geometry, instances, textures and opaque shadows"
    );
    let flat = draw(3.8, 33.0, false, false, false, "mirror")?;
    assert_ne!(patch(&tiled, 359..500), patch(&flat, 359..500));
    assert_eq!(flat.gpu, initial.gpu);

    let hole = draw(1.5, 0.0, true, true, false, "repeat")?;
    assert_eq!(hole.picked, "backdrop");
    assert_ne!(patch(&hole, 145..285), patch(&initial, 145..285));
    assert!(hole.gpu[2] > initial.gpu[2] && hole.gpu[3] > initial.gpu[3]);
    assert_eq!(hole.gpu[0], initial.gpu[0]);
    let gltf = draw(1.5, 0.0, true, false, true, "repeat")?;
    assert_ne!(patch(&gltf, 359..500), patch(&initial, 359..500));
    let gltf_flat = draw(1.5, 0.0, false, false, true, "repeat")?;
    assert_ne!(patch(&gltf, 359..500), patch(&gltf_flat, 359..500));
    assert_eq!(
        gltf.gpu, gltf_flat.gpu,
        "the normal map never creates new imported vertex buffers"
    );
    Ok(())
}

#[test]
fn authored_pbr_materials_share_geometry_and_match_the_interpreter() -> TestResult {
    use authored_material_ui::AuthoredMaterialDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/authored_material.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("AuthoredMaterialDemo")?;
    let app = AuthoredMaterialDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    struct Frame {
        viewport: Vec<u8>,
        picked: String,
        uploads: [u64; 3],
        draw: SceneDrawStats,
        atlas: Mesh3D,
    }
    let mut draw =
        |roughness: f64, metalness: f64, mode: &str, enabled: bool| -> TestResult<Frame> {
            let mut app = weak.upgrade().ok_or("missing material demo")?;
            app.set_roughness(roughness);
            app.set_metalness(metalness);
            app.set_mode(mode.into());
            app.set_style_import(enabled);
            for (key, value) in [
                ("roughness", Value::Float(roughness)),
                ("metalness", Value::Float(metalness)),
                ("mode", Value::String(mode.into())),
                ("style-import", Value::Bool(enabled)),
            ] {
                dynamic.set(key, value)?;
            }
            let png = aot.render(&mut gpu)?.to_vec();
            assert_eq!(png, interpreted.render(&mut preview_gpu)?);
            let picked = app.get_picked().to_string();
            assert_eq!(dynamic.get("picked")?, Value::String(picked.clone()));
            let (viewport, _) = inspect(&aot)?;
            let Some(Value::SceneObjects(model)) = viewport.properties.get("model") else {
                return Err("missing material scene".into());
            };
            let atlas = model
                .iter()
                .find(|object| {
                    object.id == "atlas"
                        && object
                            .mesh
                            .materials()
                            .iter()
                            .any(|material| material.base_texture.is_some())
                })
                .ok_or("atlas lost its original texture")?
                .mesh
                .clone();
            let raster = xrack_assets::decode_png(&png)?;
            Ok(Frame {
                viewport: raster.rgba[..646 * 322 * 4].to_vec(),
                picked,
                uploads: [
                    gpu.scene_mesh_uploads(),
                    gpu.scene_instance_uploads(),
                    gpu.scene_shadow_passes(),
                ],
                draw: gpu.scene_draw_stats(),
                atlas,
            })
        };
    let atlas_area = |frame: &Frame| {
        frame
            .viewport
            .chunks_exact(646 * 4)
            .skip(90)
            .take(130)
            .flat_map(|row| row[360 * 4..500 * 4].iter().copied())
            .collect::<Vec<_>>()
    };
    let initial = draw(0.26, 0.46, "opaque", true)?;
    assert_eq!(initial.picked, "sample");
    let rough = draw(0.73, 0.46, "opaque", true)?;
    assert_ne!(rough.viewport, initial.viewport);
    assert_eq!(
        rough.uploads, initial.uploads,
        "material factors keep geometry, transforms and shadows"
    );
    let metal = draw(0.73, 0.99, "opaque", true)?;
    assert_ne!(metal.viewport, rough.viewport);
    assert_eq!(metal.uploads, initial.uploads);

    let disabled = draw(0.73, 0.99, "opaque", false)?;
    assert_ne!(atlas_area(&disabled), atlas_area(&metal));
    let (Mesh3D::Imported(styled), Mesh3D::Imported(original)) = (&metal.atlas, &disabled.atlas)
    else {
        return Err("missing imported atlas data".into());
    };
    assert!(std::sync::Arc::ptr_eq(
        styled.geometry(),
        original.geometry()
    ));
    assert_eq!(metal.uploads, disabled.uploads);
    for (styled, original) in metal
        .atlas
        .materials()
        .iter()
        .zip(disabled.atlas.materials())
    {
        if let Some((a, b)) = styled
            .base_texture
            .as_ref()
            .zip(original.base_texture.as_ref())
        {
            assert!(std::sync::Arc::ptr_eq(&a.image, &b.image));
        }
    }

    let cutout = draw(0.26, 0.46, "mask", true)?;
    assert_ne!(cutout.viewport, initial.viewport);
    assert_eq!(cutout.picked, "wall");
    let blend = draw(0.26, 0.46, "blend", true)?;
    assert_ne!(blend.viewport, cutout.viewport);
    assert_eq!(blend.picked, "sample");
    let glass = draw(0.32, 0.46, "glass", true)?;
    assert_ne!(glass.viewport, blend.viewport);
    assert_eq!(glass.picked, "sample");
    assert!(glass.draw.transmission_captures > blend.draw.transmission_captures);
    assert_eq!(
        glass.uploads[0], initial.uploads[0],
        "all modes share the original geometry"
    );
    let mut app = weak.upgrade().ok_or("missing material demo")?;
    app.set_glossy(true);
    dynamic.set("glossy", Value::Bool(true))?;
    let glossy = draw(0.32, 0.46, "opaque", true)?;
    let imported = glossy
        .atlas
        .materials()
        .iter()
        .find_map(|material| material.pbr.as_ref())
        .ok_or("missing imported glossy material")?;
    assert_eq!(
        imported.workflow,
        xrack_runtime::scene3d::MeshWorkflow::SpecularGlossiness
    );
    assert!((imported.roughness - 0.06).abs() < 1e-6);
    let packed = imported
        .surface_texture
        .as_ref()
        .ok_or("missing glTF glossiness texture")?;
    let specular = imported
        .dielectric
        .as_ref()
        .and_then(|dielectric| dielectric.color_texture.as_ref())
        .ok_or("missing glTF specular color texture")?;
    assert!(std::sync::Arc::ptr_eq(&specular.image, &packed.image));
    app.set_glossiness(0.2);
    dynamic.set("glossiness", Value::Float(0.2))?;
    let matte = draw(0.32, 0.46, "opaque", true)?;
    assert_ne!(matte.viewport, glossy.viewport);
    assert_eq!(matte.uploads, glossy.uploads);
    assert_eq!(
        matte.draw.scene_image_uploads,
        glossy.draw.scene_image_uploads
    );
    app.set_glossiness(0.86);
    app.set_glossiness_channel("r".into());
    dynamic.set("glossiness", Value::Float(0.86))?;
    dynamic.set("glossiness-channel", Value::String("r".into()))?;
    let channel_r = draw(0.32, 0.46, "opaque", true)?;
    assert_ne!(channel_r.viewport, glossy.viewport);
    assert_eq!(channel_r.uploads, glossy.uploads);
    assert_eq!(
        channel_r.draw.scene_image_uploads,
        glossy.draw.scene_image_uploads
    );
    Ok(())
}

#[test]
fn filtered_picks_agree_in_both_backends_and_preserve_row_identity() -> TestResult {
    use filtered_picking_ui::FilteredPickingDemo;
    use xrack_runtime::{Hit3D, Length, Point2D};

    const NAME: &str = "examples/puzzle3d/filtered_picking.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("FilteredPickingDemo")?;
    let mut app = FilteredPickingDemo::new();
    let point = |x, y| Point2D::new(Length(x), Length(y));
    let mut check = |orbit, rear_only, mask, row, candidates: &[i64], pointer| -> TestResult<_> {
        app.set_orbit(orbit);
        app.set_rear_only(rear_only);
        app.set_mask(mask);
        app.set_target_row(row);
        app.set_candidates(candidates.to_vec());
        app.set_pointer(pointer);
        for (name, value) in [
            ("orbit", Value::Bool(orbit)),
            ("rear-only", Value::Bool(rear_only)),
            ("mask", Value::Int(mask)),
            ("target-row", Value::Int(row)),
            ("candidates", Value::IntList(candidates.to_vec())),
            ("pointer", Value::Point2D(pointer)),
        ] {
            dynamic.set(name, value)?;
        }
        let hits = app.get_hits();
        let ray_hits = app.get_ray_hits();
        let target = app.get_target();
        let ray_target = app.get_ray_target();
        for (property, got) in [("hits", &hits), ("ray-hits", &ray_hits)] {
            let Value::Hit3DList(preview_hits) = dynamic.get(property)? else {
                return Err("filtered hits did not produce a hit list".into());
            };
            assert_eq!(&preview_hits.snapshot(), got, "{property}");
        }
        assert_eq!(dynamic.get("target")?, Value::Hit3D(target.clone()));
        assert_eq!(dynamic.get("ray-target")?, Value::Hit3D(ray_target.clone()));
        let rows = |hits: Vec<Hit3D>| {
            hits.into_iter()
                .map(|hit| hit.row_index)
                .collect::<Vec<_>>()
        };
        Ok((rows(hits), target, rows(ray_hits), ray_target))
    };
    let center = point(269.0, 166.0);
    for orbit in [false, true] {
        let (hits, target, ray, ray_target) = check(orbit, true, 1, 2, &[1, 2], center)?;
        assert_eq!(hits, [2]);
        assert_eq!(ray, [2]);
        assert_eq!(target.object_id, "rear");
        assert_eq!(ray_target.object_id, "rear");
        let (hits, target, ray, ray_target) =
            check(orbit, false, 1, 2, &[2, 1, 2, 999, -1], center)?;
        assert_eq!(hits, [1, 2]);
        assert_eq!(ray, [1, 2]);
        assert_eq!(target.row_index, 2);
        assert_eq!(ray_target.row_index, 2);
        let (hits, target, ray, ray_target) = check(orbit, false, 0, 2, &[1, 2], center)?;
        assert!(hits.is_empty() && ray.is_empty() && !target.valid && !ray_target.valid);
    }
    let (hits, target, _, ray_target) = check(false, false, 3, 3, &[3, 3], point(408.0, 166.0))?;
    assert_eq!(hits, [3]);
    assert!(target.valid && target.object_id.is_empty() && !ray_target.valid);
    let (hits, target, ray, ray_target) = check(false, false, 1, -1, &[], center)?;
    assert!(hits.is_empty() && ray.is_empty() && !target.valid && !ray_target.valid);

    // Compare generated and interpreted viewport feedback for the same pick.
    check(false, true, 1, 2, &[1, 2], center)?;
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let image = aot.render(&mut gpu)?.to_vec();
    assert_eq!(image, interpreted.render(&mut gpu)?);
    Ok(())
}

#[test]
fn marquee_selects_occluded_rows_once_and_tracks_drag_in_both_backends() -> TestResult {
    use marquee_ui::MarqueeDemo;
    use xrack_runtime::{Length, Point2D, PointerButton, PointerEvent};

    const NAME: &str = "examples/puzzle3d/marquee.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MarqueeDemo")?;
    let app = MarqueeDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let selected = || -> TestResult<Vec<i64>> {
        let aot_rows = weak.upgrade().ok_or("missing marquee demo")?.get_selected();
        let preview_rows = match dynamic.get("selected")? {
            Value::IntModel(rows) => rows.snapshot(),
            Value::IntList(rows) => rows,
            _ => return Err("marquee selection was not an integer list".into()),
        };
        assert_eq!(aot_rows, preview_rows);
        Ok(aot_rows)
    };
    let point = |x, y| Point2D::new(Length(x), Length(y));
    let set = |named, mask, start, end| -> TestResult {
        let mut app = weak.upgrade().ok_or("missing marquee demo")?;
        app.set_named_camera(named);
        app.set_mask(mask);
        app.set_start(start);
        app.set_end(end);
        for (name, value) in [
            ("named-camera", Value::Bool(named)),
            ("mask", Value::Int(mask)),
            ("start", Value::Point2D(start)),
            ("end", Value::Point2D(end)),
        ] {
            dynamic.set(name, value)?;
        }
        Ok(())
    };
    assert!(selected()?.is_empty());
    // A reversed rectangle clipped to the viewport covers the visible objects,
    // including an anonymous sphere and both parts of one imported row.
    set(true, 3, point(760.0, 400.0), point(-20.0, -20.0))?;
    assert_eq!(selected()?, [1, 2, 3, 4]);
    set(true, 1, point(0.0, 0.0), point(634.0, 350.0))?;
    assert_eq!(selected()?, [1, 2, 4]);
    set(true, 2, point(0.0, 0.0), point(634.0, 350.0))?;
    assert_eq!(selected()?, [3]);
    set(true, 0, point(0.0, 0.0), point(634.0, 350.0))?;
    assert!(selected()?.is_empty());
    set(true, 3, point(248.0, 140.0), point(248.0, 200.0))?;
    assert!(selected()?.is_empty());
    set(false, 3, point(228.0, 144.0), point(276.0, 212.0))?;
    let orbit = selected()?;
    assert!(orbit.contains(&1) && orbit.contains(&2), "{orbit:?}");
    assert!(!orbit.contains(&3) && !orbit.contains(&5));

    set(true, 3, point(312.0, 63.0), point(317.0, 68.0))?;
    for (time, expected) in [(0.0, &[4][..]), (0.83, &[][..])] {
        weak.upgrade().ok_or("missing marquee demo")?.set_time(time);
        dynamic.set("time", Value::Float(time))?;
        assert_eq!(selected()?, expected);
    }
    set(true, 3, point(0.0, 0.0), point(0.0, 0.0))?;
    let initial = aot.render(&mut gpu)?.to_vec();
    assert_eq!(initial, interpreted.render(&mut gpu)?);
    let start = Point { x: 220.0, y: 143.0 };
    let end = Point { x: 274.0, y: 208.0 };
    let contact = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent {
            button: PointerButton::Primary,
            ..Default::default()
        },
    };
    aot.press_contact(contact, start, false);
    interpreted.press_contact(contact, start, false);
    aot.move_contact(contact, end);
    interpreted.move_contact(contact, end);
    assert_eq!(selected()?, [1, 2]);
    let dragging = aot.render(&mut gpu)?.to_vec();
    assert_ne!(dragging, initial);
    assert_eq!(dragging, interpreted.render(&mut gpu)?);
    aot.finish_contact(contact, end, false, false);
    interpreted.finish_contact(contact, end, false, false);
    assert_eq!(selected()?, [1, 2]);
    let finished = aot.render(&mut gpu)?.to_vec();
    assert_ne!(finished, dragging);
    assert_eq!(finished, interpreted.render(&mut gpu)?);
    Ok(())
}

#[test]
fn closest_point_queries_track_pose_target_and_layers_in_both_backends() -> TestResult {
    use closest_surface_ui::ClosestSurfaceDemo;
    use xrack_runtime::Hit3D;

    const NAME: &str = "examples/puzzle3d/closest_surface.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ClosestSurfaceDemo")?;
    let mut app = ClosestSurfaceDemo::new();
    #[derive(Clone, Copy, Debug)]
    struct Query<'a> {
        x: f64,
        radius: f64,
        target: &'a str,
        time: f64,
        layers: i64,
        skin: bool,
        visible: bool,
        pickable: bool,
        yaw: f64,
    }
    let base = Query {
        x: 0.25,
        radius: 2.2,
        target: "",
        time: 0.0,
        layers: 1,
        skin: false,
        visible: true,
        pickable: true,
        yaw: 0.0,
    };
    let mut query = |query: Query<'_>| -> TestResult<Hit3D> {
        app.set_probe_x(query.x);
        app.set_reach(query.radius);
        app.set_target(query.target.into());
        app.set_time(query.time);
        app.set_layers(query.layers);
        app.set_skin(query.skin);
        app.set_visible_wall(query.visible);
        app.set_pickable_wall(query.pickable);
        app.set_wall_yaw(query.yaw);
        for (name, value) in [
            ("probe-x", Value::Float(query.x)),
            ("reach", Value::Float(query.radius)),
            ("target", Value::String(query.target.into())),
            ("time", Value::Float(query.time)),
            ("layers", Value::Int(query.layers)),
            ("skin", Value::Bool(query.skin)),
            ("visible-wall", Value::Bool(query.visible)),
            ("pickable-wall", Value::Bool(query.pickable)),
            ("wall-yaw", Value::Float(query.yaw)),
        ] {
            dynamic.set(name, value)?;
        }
        let result = app.get_result();
        assert_eq!(dynamic.get("result")?, Value::Hit3D(result.clone()));
        Ok(result)
    };
    let near = |actual: f64, expected: f64| {
        assert!((actual - expected).abs() < 1e-3, "{actual} != {expected}");
    };
    let wall = query(base)?;
    assert!(wall.valid);
    assert_eq!((&*wall.object_id, wall.row_index), ("wall", 1));
    near(wall.position.x, 0.25);
    near(wall.position.y, 0.3);
    near(wall.position.z, 0.45);
    near(wall.local_position.z, 0.5);
    near(wall.distance, 1.4);
    near(wall.normal.z, 1.0);

    let tilted = query(Query { yaw: 30.0, ..base })?;
    assert!(tilted.valid);
    near(tilted.distance, 1.85 * 30.0_f64.to_radians().cos() - 0.45);
    near(tilted.normal.x.abs(), 0.5);
    near(tilted.normal.z, 30.0_f64.to_radians().cos());

    for case in [
        Query { layers: 0, ..base },
        Query {
            radius: 1.39,
            ..base
        },
        Query {
            radius: -1.0,
            ..base
        },
        Query {
            pickable: false,
            radius: 1.6,
            ..base
        },
        Query {
            visible: false,
            radius: 1.6,
            ..base
        },
        Query {
            target: "missing",
            ..base
        },
        Query {
            target: "beacon",
            ..base
        },
    ] {
        assert_eq!(query(case)?, Hit3D::default(), "{case:?}");
    }

    let beacon = query(Query {
        target: "beacon",
        layers: 3,
        x: 2.7,
        ..base
    })?;
    assert!(beacon.valid && beacon.object_id == "beacon" && beacon.row_index == 2);
    near(beacon.position.z, 0.72);
    near(beacon.distance, 1.13);

    for skin in [false, true] {
        let first = Query {
            target: "panel",
            x: if skin { -3.55 } else { -2.02 },
            radius: 4.0,
            skin,
            ..base
        };
        let initial = query(first)?;
        let moved = query(Query {
            time: 0.83,
            ..first
        })?;
        assert!(initial.valid && moved.valid && moved.object_id == "panel");
        assert!(
            (initial.distance - moved.distance).abs() > 0.02,
            "initial={initial:?}, moved={moved:?}, skin={skin}"
        );
        assert_ne!(initial.local_position, moved.local_position);
        assert!((0.0..=1.0).contains(&moved.u) && (0.0..=1.0).contains(&moved.v));
        assert_eq!(query(first)?, initial);
    }
    Ok(())
}

#[test]
fn model_particles_use_geometry_materials_and_depth_in_both_backends() -> TestResult {
    use particle_models_ui::ParticleModelsDemo;

    const NAME: &str = "examples/puzzle3d/particle_models.xr";
    let text = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&text, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("ParticleModelsDemo")?;
    let app = ParticleModelsDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        time: f64,
        imported: bool,
        spin: bool,
        cover: bool,
        translucent: bool,
        light: f64,
    }
    let base = State {
        time: 0.7,
        imported: false,
        spin: true,
        cover: false,
        translucent: false,
        light: 35.0,
    };
    #[derive(Debug)]
    struct Work {
        draws: u64,
        models: u64,
        sprites: u64,
        transparency: u64,
        mesh_uploads: u64,
    }
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing model particle demo")?;
        app.set_time(state.time);
        app.set_imported(state.imported);
        app.set_spin(state.spin);
        app.set_cover(state.cover);
        app.set_translucent(state.translucent);
        app.set_light_angle(state.light);
        for (name, value) in [
            ("time", Value::Float(state.time)),
            ("imported", Value::Bool(state.imported)),
            ("spin", Value::Bool(state.spin)),
            ("cover", Value::Bool(state.cover)),
            ("translucent", Value::Bool(state.translucent)),
            ("light-angle", Value::Float(state.light)),
        ] {
            interpreted.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let uploads = gpu.scene_mesh_uploads();
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == dynamic.render(&mut other_gpu)?);
        let after = gpu.scene_draw_stats();
        Ok((
            xrack_assets::decode_png(&png)?,
            Work {
                draws: after.particle_model_draws - before.particle_model_draws,
                models: after.particle_model_instances - before.particle_model_instances,
                sprites: after.particle_draws - before.particle_draws,
                transparency: after.transparency_composites - before.transparency_composites,
                mesh_uploads: gpu.scene_mesh_uploads() - uploads,
            },
        ))
    };
    let region = |image: &xrack_assets::Raster, left: bool| -> Vec<[u8; 4]> {
        let range = if left { 166..345 } else { 346..513 };
        (80..226)
            .flat_map(|y| {
                range
                    .clone()
                    .map(move |x| image.rgba.as_chunks::<4>().0[x + y * 670])
            })
            .collect()
    };
    let cyan = |pixels: &[[u8; 4]]| {
        pixels
            .iter()
            .filter(|p| p[1] > 98 && p[2] > 130 && 2 * u16::from(p[0]) < u16::from(p[2]))
            .count()
    };
    let (solid, stats) = render(base)?;
    assert_eq!(
        (stats.draws, stats.models, stats.sprites, stats.transparency),
        (1, 2, 0, 0)
    );
    assert_eq!(
        render(base)?.1.models,
        0,
        "a stationary viewport reuses its frame"
    );
    let left = region(&solid, true);
    assert!(cyan(&left) > 150);
    let stationary = render(State {
        spin: false,
        ..base
    })?
    .0;
    assert_ne!(
        left,
        region(&stationary, true),
        "particle rotation changes the model silhouette"
    );
    let relit = render(State {
        light: -135.0,
        ..base
    })?
    .0;
    assert_ne!(
        left,
        region(&relit, true),
        "particle normals use the viewport lighting"
    );

    let hidden = render(State {
        cover: true,
        ..base
    })?
    .0;
    assert!(cyan(&region(&hidden, true)) < cyan(&left) / 6);
    assert_eq!(
        region(&solid, false),
        region(&hidden, false),
        "the cover only occludes the nearby particle"
    );
    let (faded, stats) = render(State {
        translucent: true,
        ..base
    })?;
    assert_eq!((stats.models, stats.draws, stats.transparency), (2, 2, 1));
    assert_ne!(region(&faded, true), left);
    assert_eq!(region(&faded, false), region(&solid, false));

    let moved = render(State { time: 1.14, ..base })?.0;
    assert_ne!(region(&moved, true), left);
    assert_eq!(render(base)?.0.rgba, solid.rgba);

    let imported = State {
        imported: true,
        ..base
    };
    let (model, stats) = render(imported)?;
    assert!(stats.mesh_uploads > 0 && stats.draws > 0);
    assert_ne!(region(&model, true), left);
    let (model_later, later_stats) = render(State {
        time: 1.16,
        ..imported
    })?;
    assert_ne!(model_later.rgba, model.rgba);
    assert_eq!(
        later_stats.mesh_uploads, 0,
        "animation only changes instance transforms"
    );
    assert_eq!(render(imported)?.0.rgba, model.rgba);
    Ok(())
}

#[test]
fn model_particle_shadows_and_sliced_reflections_match_in_both_backends() -> TestResult {
    use particle_lighting_ui::ParticleLightingDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/particle_lighting.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ParticleLightingDemo")?;
    let app = ParticleLightingDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let work = |earlier: SceneDrawStats, later: SceneDrawStats| {
        [
            later.particle_shadow_draws - earlier.particle_shadow_draws,
            later.particle_reflection_instances - earlier.particle_reflection_instances,
            later.reflection_capture_faces - earlier.reflection_capture_faces,
            later.reflection_cubemap_updates - earlier.reflection_cubemap_updates,
        ]
    };
    let mut render =
        |time: f64, cast: bool, reflect: bool, receive: bool, slicing: &str| -> TestResult<_> {
            let mut app = weak.upgrade().ok_or("missing particle lighting demo")?;
            app.set_time(time);
            app.set_cast_shadow(cast);
            app.set_cast_reflection(reflect);
            app.set_receive_shadow(receive);
            app.set_slicing(slicing.into());
            for (name, value) in [
                ("time", Value::Float(time)),
                ("cast-shadow", Value::Bool(cast)),
                ("cast-reflection", Value::Bool(reflect)),
                ("receive-shadow", Value::Bool(receive)),
                ("slicing", Value::String(slicing.into())),
            ] {
                dynamic.set(name, value)?;
            }
            let old_aot = gpu.scene_draw_stats();
            let old_preview = preview_gpu.scene_draw_stats();
            let png = aot.render_animated(&mut gpu)?.to_vec();
            assert!(
                png == interpreted.render_animated(&mut preview_gpu)?,
                "{time} {slicing}"
            );
            let counters = work(old_aot, gpu.scene_draw_stats());
            assert_eq!(counters, work(old_preview, preview_gpu.scene_draw_stats()));
            Ok((xrack_assets::decode_png(&png)?, counters))
        };
    let region =
        |image: &xrack_assets::Raster, x: std::ops::Range<usize>, y: std::ops::Range<usize>| {
            let pixels = image.rgba.as_chunks::<4>().0;
            y.flat_map(|row| {
                pixels[row * 600 + x.start..row * 600 + x.end]
                    .iter()
                    .copied()
            })
            .collect::<Vec<_>>()
        };
    let mirror = |image: &xrack_assets::Raster| region(image, 382..471, 113..196);
    let wall = |image: &xrack_assets::Raster| region(image, 225..350, 165..276);

    let (unlit, initial) = render(0.65, false, false, false, "none")?;
    assert_eq!(initial, [0, 0, 6, 1]);
    let (lit, with_shadows) = render(0.65, true, false, false, "none")?;
    assert_eq!(with_shadows, [1, 0, 0, 0]);
    let darkened = wall(&unlit)
        .iter()
        .zip(wall(&lit))
        .filter(|(before, after)| before[0] > after[0].saturating_add(20))
        .count();
    assert!(
        darkened > 150,
        "particle shadow darkened {darkened} wall pixels"
    );
    let (moving, updated) = render(1.2, true, false, false, "none")?;
    assert_eq!(updated, [1, 0, 0, 0]);
    assert!(wall(&moving) != wall(&lit));
    assert_eq!(render(1.2, true, false, false, "none")?.1, [0, 0, 0, 0]);

    let (near, reflected) = render(0.65, false, true, true, "none")?;
    assert_eq!(
        reflected,
        [0, 1, 6, 1],
        "six faces share one instance stream"
    );
    assert!(
        mirror(&near) != mirror(&unlit),
        "the particle appears in the mirror"
    );
    let (far, moved_reflection) = render(1.6, false, true, true, "none")?;
    assert_eq!(moved_reflection, [0, 1, 6, 1]);
    assert!(mirror(&near) != mirror(&far));

    // The second face onward sees a newer app time. Publish all six faces
    // from the same first sample, then converge to the newer snapshot.
    for step in 0..24 {
        let time = if step == 0 { 0.65 } else { 1.6 };
        let (frame, counters) = render(time, false, true, true, "individual-faces")?;
        let slice = step % 12;
        let capture = u64::from(slice < 6);
        assert_eq!(
            counters,
            [0, capture, capture, u64::from(slice == 11)],
            "{step}"
        );
        let target = if (11..23).contains(&step) {
            &near
        } else {
            &far
        };
        assert!(
            mirror(&frame) == mirror(target),
            "incoherent particle reflection at step {step}"
        );
    }
    assert_eq!(
        render(1.6, false, true, true, "individual-faces")?.1,
        [0, 0, 0, 0]
    );
    Ok(())
}

#[test]
fn sphere_walk_steps_snaps_and_respects_slope_and_headroom_in_both_backends() -> TestResult {
    use sphere_walk_ui::SphereWalkDemo;
    use xrack_runtime::scene3d::{Point3D, Vector3D, capsule_trigger_crossings};

    const NAME: &str = "examples/puzzle3d/sphere_walk.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("SphereWalkDemo")?;
    let app = SphereWalkDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut render = || -> TestResult<Vec<u8>> {
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == interpreter.render(&mut other_gpu)?);
        Ok(xrack_assets::decode_png(&png)?.rgba[..620 * 360 * 4].to_vec())
    };
    #[derive(Clone, Copy)]
    struct Case {
        center: Point3D,
        drive: f64,
        vertical: f64,
        steps: bool,
        snapping: bool,
        ceiling: bool,
        ramp: f64,
    }
    let initial = Case {
        center: Point3D::new(-2.4, 0.4, 0.0),
        drive: 1.8,
        vertical: 0.0,
        steps: true,
        snapping: true,
        ceiling: false,
        ramp: 30.0,
    };
    let apply = |case: Case| -> TestResult<Point3D> {
        let mut app = weak.upgrade().ok_or("missing grounded movement example")?;
        app.set_center(case.center);
        app.set_drive(case.drive);
        app.set_vertical(case.vertical);
        app.set_steps(case.steps);
        app.set_snapping(case.snapping);
        app.set_ceiling(case.ceiling);
        app.set_ramp_angle(case.ramp);
        for (name, value) in [
            ("center", Value::Point3D(case.center)),
            ("drive", Value::Float(case.drive)),
            ("vertical", Value::Float(case.vertical)),
            ("steps", Value::Bool(case.steps)),
            ("snapping", Value::Bool(case.snapping)),
            ("ceiling", Value::Bool(case.ceiling)),
            ("ramp-angle", Value::Float(case.ramp)),
        ] {
            dynamic.set(name, value)?;
        }
        let endpoint = app.get_endpoint();
        assert_eq!(dynamic.get("endpoint")?, Value::Point3D(endpoint));
        Ok(endpoint)
    };
    let climbed = apply(initial)?;
    assert!((climbed.x + 0.6).abs() < 1e-8, "{climbed:?}");
    assert!((0.64..0.641).contains(&climbed.y), "{climbed:?}");
    let raised_image = render()?;
    let blocked = apply(Case {
        steps: false,
        ..initial
    })?;
    assert!(
        blocked.x < -0.95 && (blocked.y - 0.4).abs() < 1e-6,
        "{blocked:?}"
    );
    assert!(render()? != raised_image);

    let headroom = apply(Case {
        ceiling: true,
        ..initial
    })?;
    assert!(headroom.y < 0.41 && headroom.x < -0.95, "{headroom:?}");
    let deck = Case {
        center: climbed,
        drive: 1.3,
        ..initial
    };
    let descended = apply(deck)?;
    assert!((descended.x - 0.7).abs() < 1e-8, "{descended:?}");
    assert!((0.4..0.401).contains(&descended.y), "{descended:?}");
    let stayed_high = apply(Case {
        snapping: false,
        ..deck
    })?;
    assert!((stayed_high.y - climbed.y).abs() < 1e-8, "{stayed_high:?}");

    let floating = apply(Case {
        center: Point3D::new(-3.0, 0.6, 0.0),
        drive: 0.35,
        ..initial
    })?;
    assert!((floating.y - 0.6).abs() < 1e-8, "{floating:?}");
    let jumping = apply(Case {
        drive: -0.35,
        vertical: 0.2,
        ..initial
    })?;
    assert!((jumping.y - 0.6).abs() < 1e-8, "{jumping:?}");

    let slope = Case {
        center: Point3D::new(-0.65, 0.4, -2.2),
        drive: 2.0,
        steps: false,
        ..initial
    };
    let gentle = apply(slope)?;
    assert!(gentle.y > 0.6 && gentle.x > 0.8, "{gentle:?}");
    let steep = apply(Case {
        ramp: 65.0,
        ..slope
    })?;
    assert!(steep.y < 0.41 && steep.x < 1.2, "{steep:?}");
    let still = apply(Case {
        drive: 0.0,
        ..initial
    })?;
    assert_eq!(still, initial.center);

    let mut app = weak.upgrade().ok_or("missing capsule movement example")?;
    app.set_upright(true);
    dynamic.set("upright", Value::Bool(true))?;
    let blocked = apply(Case {
        center: Point3D::new(-2.4, 0.85, 0.0),
        ..initial
    })?;
    assert!(blocked.x < -0.9 && blocked.x > -1.5, "{blocked:?}");
    assert!(render()? != raised_image);
    let capsule_slope = Case {
        center: Point3D::new(-0.65, 0.85, -2.2),
        ..slope
    };
    let climbed_slope = apply(capsule_slope)?;
    assert!(
        climbed_slope.y > 1.05 && climbed_slope.x > 0.8,
        "{climbed_slope:?}"
    );
    let stood_on_steep_ground = apply(Case {
        ramp: 65.0,
        ..capsule_slope
    })?;
    assert!(stood_on_steep_ground.y < 0.86, "{stood_on_steep_ground:?}");

    let probe = Point3D::new(-2.4, 1.22, 0.0);
    app.set_center(probe);
    dynamic.set("center", Value::Point3D(probe))?;
    let hit = app.get_first_contact();
    assert_eq!(Value::Hit3D(hit.clone()), dynamic.get("first-contact")?);
    assert_eq!(hit.object_id, "beam");
    assert!((hit.distance - 1.47).abs() < 1e-5, "{hit:?}");
    assert!((hit.position.x + 0.53).abs() < 1e-5);
    let slide = app.get_slide_target();
    assert_eq!(Value::Point3D(slide), dynamic.get("slide-target")?);
    assert!((-0.931..=-0.929).contains(&slide.x), "{slide:?}");

    let inside = Point3D::new(-0.4, 1.22, 0.0);
    app.set_center(inside);
    dynamic.set("center", Value::Point3D(inside))?;
    let Value::Hit3DList(contacts) = dynamic.get("contacts")? else {
        return Err("missing capsule overlaps".into());
    };
    assert_eq!(contacts.len(), 1);
    let contact = contacts.row(0).ok_or("missing beam overlap")?;
    assert_eq!(contact.object_id, "beam");
    assert_eq!(app.get_contacts(), vec![contact]);

    let start = Point3D::new(-2.4, 0.85, 0.0);
    let middle = Point3D::new(-1.8, 0.85, 0.0);
    let end = Point3D::new(-1.0, 0.85, 0.0);
    app.set_center(start);
    dynamic.set("center", Value::Point3D(start))?;
    for (point, expected) in [
        (middle, [1, 0, 1]),
        (middle, [1, 0, 1]),
        (Point3D::new(-1.65, 0.85, 0.0), [1, 0, 1]),
        (end, [1, 1, 0]),
        (start, [2, 2, 0]),
    ] {
        app.invoke_commit_move(point);
        dynamic.invoke("commit-move", &[Value::Point3D(point)])?;
        assert_eq!(
            [
                app.get_entries(),
                app.get_exits(),
                app.get_active_triggers()
            ],
            expected
        );
        for (name, count) in ["entries", "exits", "active-triggers"]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(dynamic.get(name)?, Value::Int(count), "{name}");
        }
    }
    app.set_upright(false);
    dynamic.set("upright", Value::Bool(false))?;
    app.set_steps(true);
    dynamic.set("steps", Value::Bool(true))?;
    app.set_center(initial.center);
    dynamic.set("center", Value::Point3D(initial.center))?;
    app.set_entries(0);
    app.set_exits(0);
    dynamic.set("entries", Value::Int(0))?;
    dynamic.set("exits", Value::Int(0))?;
    let direction = Vector3D::new(initial.drive, 0.0, 0.0);
    app.invoke_commit_walk(direction);
    dynamic.invoke("commit-walk", &[Value::Vector3D(direction)])?;
    assert_eq!(app.get_center(), climbed);
    assert_eq!(dynamic.get("center")?, Value::Point3D(climbed));
    assert_eq!([app.get_entries(), app.get_exits()], [1, 1]);
    assert_eq!(dynamic.get("entries")?, Value::Int(1));
    assert_eq!(dynamic.get("exits")?, Value::Int(1));
    assert!(app.get_grounded() && !app.get_hit_wall() && !app.get_hit_ceiling());
    assert_eq!(dynamic.get("grounded")?, Value::Bool(true));
    assert_eq!(dynamic.get("hit-wall")?, Value::Bool(false));
    assert_eq!(dynamic.get("hit-ceiling")?, Value::Bool(false));
    let landed_on_step = app.get_last_shape_hit();
    assert_eq!(landed_on_step.object_id, "step");
    assert!(landed_on_step.normal.y > 0.999);
    assert_eq!(dynamic.get("last-shape-hit")?, Value::Hit3D(landed_on_step));

    let support = app.get_ground_contact();
    assert!(support.valid && support.normal.y > 0.999);
    assert_eq!(support.object_id, "step");
    assert_eq!(dynamic.get("ground-contact")?, Value::Hit3D(support));
    for delta in [Vector3D::new(0.0, 0.14, 0.0), Vector3D::new(0.0, 0.0, 0.45)] {
        let center = app.get_center();
        let platform = app.get_platform_position();
        app.invoke_move_platform(delta);
        dynamic.invoke("move-platform", &[Value::Vector3D(delta)])?;
        assert!(!app.get_platform_blocked());
        assert_eq!(dynamic.get("platform-blocked")?, Value::Bool(false));
        assert_eq!(
            app.get_center(),
            Point3D::new(center.x + delta.x, center.y + delta.y, center.z + delta.z)
        );
        assert_eq!(
            app.get_platform_position(),
            Point3D::new(
                platform.x + delta.x,
                platform.y + delta.y,
                platform.z + delta.z
            )
        );
        assert_eq!(dynamic.get("center")?, Value::Point3D(app.get_center()));
        assert_eq!(
            dynamic.get("platform-position")?,
            Value::Point3D(app.get_platform_position())
        );
        assert_eq!(app.get_ground_contact().object_id, "step");
        assert_eq!(
            dynamic.get("ground-contact")?,
            Value::Hit3D(app.get_ground_contact())
        );
        let _ = render()?;
    }
    let center = app.get_center();
    let platform = app.get_platform_position();
    let blocked = Vector3D::new(0.0, 0.75, 0.0);
    app.invoke_move_platform(blocked);
    dynamic.invoke("move-platform", &[Value::Vector3D(blocked)])?;
    assert!(app.get_platform_blocked());
    assert_eq!(dynamic.get("platform-blocked")?, Value::Bool(true));
    assert_eq!(app.get_center(), center);
    assert_eq!(app.get_platform_position(), platform);
    assert_eq!(dynamic.get("center")?, Value::Point3D(center));
    assert_eq!(dynamic.get("platform-position")?, Value::Point3D(platform));

    let air = Point3D::new(-2.0, 2.0, 0.0);
    app.set_center(air);
    dynamic.set("center", Value::Point3D(air))?;
    app.invoke_move_platform(blocked);
    dynamic.invoke("move-platform", &[Value::Vector3D(blocked)])?;
    assert!(!app.get_ground_contact().valid);
    assert_eq!(app.get_center(), air);
    assert_eq!(app.get_platform_position(), platform);
    assert_eq!(dynamic.get("center")?, Value::Point3D(air));
    assert_eq!(dynamic.get("platform-position")?, Value::Point3D(platform));

    for ceiling in [false, true] {
        app.set_ceiling(ceiling);
        dynamic.set("ceiling", Value::Bool(ceiling))?;
        app.invoke_reset_walk();
        dynamic.invoke("reset-walk", &[])?;
        app.invoke_jump();
        dynamic.invoke("jump", &[])?;
        assert!(app.get_jumping());
        let mut apex = app.get_center().y;
        let mut touched_ceiling = false;
        for _ in 0..80 {
            if !app.get_jumping() {
                break;
            }
            app.invoke_tick_jump();
            dynamic.invoke("tick-jump", &[])?;
            assert_eq!(dynamic.get("center")?, Value::Point3D(app.get_center()));
            for (name, state) in [
                ("grounded", app.get_grounded()),
                ("hit-ceiling", app.get_hit_ceiling()),
                ("jumping", app.get_jumping()),
            ] {
                assert_eq!(dynamic.get(name)?, Value::Bool(state), "{name}");
            }
            apex = apex.max(app.get_center().y);
            if app.get_hit_ceiling() {
                let hit = app.get_last_shape_hit();
                assert_eq!(hit.object_id, "ceiling");
                assert!(hit.normal.y < -0.999);
                assert_eq!(dynamic.get("last-shape-hit")?, Value::Hit3D(hit));
                touched_ceiling = true;
            }
        }
        assert!(!app.get_jumping() && app.get_grounded());
        assert_eq!(app.get_jump_bumped(), ceiling);
        assert_eq!(dynamic.get("jump-bumped")?, Value::Bool(ceiling));
        assert_eq!(touched_ceiling, ceiling);
        let floor = app.get_last_shape_hit();
        assert_eq!(floor.object_id, "floor");
        assert!(floor.normal.y > 0.999);
        assert_eq!(dynamic.get("last-shape-hit")?, Value::Hit3D(floor));
        assert!(
            if ceiling {
                (0.46..0.48).contains(&apex)
            } else {
                apex > 1.2
            },
            "apex: {apex}"
        );
    }
    let _ = render()?;
    let (view, _) = inspect(&aot)?;
    let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
        return Err("missing trigger scene model".into());
    };
    let events = capsule_trigger_crossings(model, start, end, 0.4, 0.9, None);
    assert_eq!(events.len(), 2);
    let entered = events.row(0).ok_or("missing entry")?;
    let exited = events.row(1).ok_or("missing exit")?;
    assert!(entered.entering && !exited.entering);
    assert_eq!(entered.object_id, "checkpoint");
    assert_eq!(exited.object_id, "checkpoint");
    assert!((entered.distance - 0.1).abs() < 1e-5);
    assert!((exited.distance - 1.1).abs() < 1e-5);
    assert!(entered.normal.x < -0.99 && exited.normal.x > 0.99);
    assert!(capsule_trigger_crossings(model, start, end, 0.4, 0.9, Some(4)).is_empty());
    Ok(())
}

#[test]
fn sphere_slide_preserves_tangent_motion_and_stops_at_a_two_wall_corner() -> TestResult {
    use sphere_slide_ui::SphereSlideDemo;
    use xrack_runtime::scene3d::Point3D;

    const NAME: &str = "examples/puzzle3d/sphere_slide.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("SphereSlideDemo")?;
    let app = SphereSlideDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let render = |aot: &mut PreviewScene,
                  interpreter: &mut PreviewScene,
                  gpu: &mut Renderer,
                  other_gpu: &mut Renderer|
     -> TestResult<Vec<u8>> {
        let png = aot.render(gpu)?.to_vec();
        assert!(png.as_slice() == interpreter.render(other_gpu)?);
        Ok(xrack_assets::decode_png(&png)?.rgba[..610 * 350 * 4].to_vec())
    };
    let mut app = weak.upgrade().ok_or("missing sphere slide demo")?;
    let check = |app: &SphereSlideDemo| -> TestResult<Point3D> {
        let end = app.get_endpoint();
        assert_eq!(Value::Point3D(end), dynamic.get("endpoint")?);
        Ok(end)
    };
    let corner = check(&app)?;
    assert!(
        (corner.x + 0.58).abs() < 1e-4 && corner.x <= -0.58,
        "{corner:?}"
    );
    assert!(
        (corner.z - 0.78).abs() < 1e-4 && corner.z <= 0.78,
        "{corner:?}"
    );
    assert_eq!(app.get_first_contact().object_id, "long-wall");
    let corner_image = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    let uploads = gpu.scene_mesh_uploads();
    app.set_corner(false);
    dynamic.set("corner", Value::Bool(false))?;
    let wall = check(&app)?;
    assert!((wall.x + 0.58).abs() < 1e-4 && wall.x <= -0.58);
    assert!((wall.z - 1.7).abs() < 1e-10, "{wall:?}");
    assert!(corner_image != render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?);
    assert_eq!(gpu.scene_mesh_uploads(), uploads);

    app.set_mask(0);
    dynamic.set("mask", Value::Int(0))?;
    let free = check(&app)?;
    assert!((free.x - 2.4).abs() < 1e-10 && (free.z - 1.7).abs() < 1e-10);

    app.set_mask(1);
    let shallow_penetration = Point3D::new(-0.56, 0.55, 0.0);
    app.set_center(shallow_penetration);
    app.set_drive_x(-0.8);
    app.set_drive_z(0.0);
    dynamic.set("mask", Value::Int(1))?;
    dynamic.set("center", Value::Point3D(shallow_penetration))?;
    dynamic.set("drive-x", Value::Float(-0.8))?;
    dynamic.set("drive-z", Value::Float(0.0))?;
    assert!((check(&app)?.x + 1.36).abs() < 1e-10);

    let inside = Point3D::new(0.0, 0.55, 0.0);
    app.set_center(inside);
    dynamic.set("center", Value::Point3D(inside))?;
    assert_eq!(check(&app)?, inside);
    Ok(())
}

#[test]
fn rigid_body_world_drives_the_same_scene_in_both_backends() -> TestResult {
    use rigid_bodies_ui::RigidBodiesDemo;

    const NAME: &str = "examples/puzzle3d/rigid_bodies.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("RigidBodiesDemo")?;
    let app = RigidBodiesDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut app = weak.upgrade().ok_or("missing rigid body demo")?;
    let mut render = || -> TestResult<Vec<u8>> {
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png.as_slice(), interpreter.render(&mut other_gpu)?);
        Ok(png)
    };

    app.invoke_reset();
    dynamic.invoke("reset", &[])?;
    assert!(app.get_first() > 0 && app.get_second() > app.get_first());
    let before = render()?;
    for _ in 0..70 {
        app.invoke_advance();
        dynamic.invoke("advance", &[])?;
    }
    assert_eq!(app.get_ticks(), 70);
    assert_eq!(dynamic.get("ticks")?, Value::Int(70));
    let landed = app.get_crate_position();
    assert!((landed.y - 0.75).abs() < 0.04, "{landed:?}");
    assert_eq!(dynamic.get("crate-position")?, Value::Point3D(landed));
    assert!(render()? != before);
    let second_before = app.get_second_position();
    app.invoke_launch();
    dynamic.invoke("launch", &[])?;
    assert!(app.get_first_velocity().x > 5.0);
    assert_eq!(
        dynamic.get("first-velocity")?,
        Value::Vector3D(app.get_first_velocity())
    );
    for _ in 0..40 {
        app.invoke_advance();
        dynamic.invoke("advance", &[])?;
    }
    let second = app.get_second_position();
    assert!(second.x > second_before.x + 0.15, "{second:?}");
    assert_eq!(dynamic.get("second-position")?, Value::Point3D(second));
    assert_eq!(
        dynamic.get("first-position")?,
        Value::Point3D(app.get_first_position())
    );
    assert_eq!(
        dynamic.get("crate-rotation")?,
        Value::Vector3D(app.get_crate_rotation())
    );
    let lowered = render()?;
    app.set_lift_raised(true);
    dynamic.set("lift-raised", Value::Bool(true))?;
    for _ in 0..60 {
        app.invoke_advance();
        dynamic.invoke("advance", &[])?;
    }
    let lift = app.get_lift_position();
    assert!((lift.y - 1.35).abs() < 0.001, "{lift:?}");
    assert_eq!(dynamic.get("lift-position")?, Value::Point3D(lift));
    let lifted = app.get_crate_position();
    assert!(lifted.y > landed.y + 1.1, "{lifted:?}");
    assert_eq!(dynamic.get("crate-position")?, Value::Point3D(lifted));
    assert_ne!(lowered, render()?);
    assert!(app.get_sensor_active());
    assert_eq!(app.get_sensor_count(), 1);
    assert_eq!(app.get_sensor_entries(), 1);
    assert_eq!(app.get_sensor_exits(), 0);
    assert_eq!(dynamic.get("sensor-active")?, Value::Bool(true));
    assert_eq!(dynamic.get("sensor-count")?, Value::Int(1));
    assert_eq!(dynamic.get("sensor-entries")?, Value::Int(1));
    app.invoke_enable_sensor(false);
    dynamic.invoke("enable-sensor", &[Value::Bool(false)])?;
    assert!(app.get_sensor_active());
    assert_eq!(dynamic.get("sensor-active")?, Value::Bool(true));
    app.invoke_advance();
    dynamic.invoke("advance", &[])?;
    assert!(!app.get_sensor_active());
    assert_eq!(app.get_sensor_exits(), 1);
    assert_eq!(dynamic.get("sensor-count")?, Value::Int(0));
    assert_eq!(dynamic.get("sensor-exits")?, Value::Int(1));
    app.invoke_enable_sensor(true);
    dynamic.invoke("enable-sensor", &[Value::Bool(true)])?;
    app.invoke_advance();
    dynamic.invoke("advance", &[])?;
    assert!(app.get_sensor_active());
    assert_eq!(app.get_sensor_entries(), 2);
    assert_eq!(dynamic.get("sensor-count")?, Value::Int(1));
    assert_eq!(dynamic.get("sensor-entries")?, Value::Int(2));
    let _ = render()?;
    app.set_lift_raised(false);
    dynamic.set("lift-raised", Value::Bool(false))?;
    for _ in 0..80 {
        app.invoke_advance();
        dynamic.invoke("advance", &[])?;
    }
    assert!(!app.get_sensor_active());
    assert_eq!(app.get_sensor_entries(), 2);
    assert_eq!(app.get_sensor_exits(), 2);
    assert_eq!(dynamic.get("sensor-active")?, Value::Bool(false));
    assert_eq!(dynamic.get("sensor-exits")?, Value::Int(2));
    let _ = render()?;
    Ok(())
}

#[test]
fn physics_character_matches_across_aot_and_interpreter_for_stairs_platform_and_push() -> TestResult
{
    use physics_character_ui::PhysicsCharacterDemo;

    const NAME: &str = "examples/puzzle3d/physics_character.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("PhysicsCharacterDemo")?;
    let demo = PhysicsCharacterDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut aot_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);
    let mut demo = weak.upgrade().ok_or("missing physics character demo")?;
    let advance = |demo: &mut PhysicsCharacterDemo, count| -> TestResult {
        for _ in 0..count {
            demo.invoke_advance();
            interpreted.invoke("advance", &[])?;
            assert_eq!(
                interpreted.get("position")?,
                Value::Point3D(demo.get_position())
            );
            assert_eq!(interpreted.get("support")?, Value::Int(demo.get_support()));
            assert_eq!(
                interpreted.get("grounded")?,
                Value::Bool(demo.get_grounded())
            );
            assert_eq!(
                interpreted.get("wall-hit")?,
                Value::Bool(demo.get_wall_hit())
            );
            let Value::PhysicsContact3DList(hits) = interpreted.get("step-hits")? else {
                return Err("expected a typed character hit list".into());
            };
            assert_eq!(demo.get_step_hits(), hits.snapshot_untracked());
        }
        Ok(())
    };
    let mut render = || -> TestResult {
        assert_eq!(
            aot_view.render(&mut aot_gpu)?,
            interpreter_view.render(&mut interpreter_gpu)?
        );
        Ok(())
    };
    demo.invoke_setup(0);
    interpreted.invoke("setup", &[Value::Int(0)])?;
    assert!(demo.get_player() > 0);
    advance(&mut demo, 80)?;
    assert_eq!(demo.get_support(), demo.get_stair());
    assert!(demo.get_position().y > 0.9, "{:?}", demo.get_position());
    render()?;
    advance(&mut demo, 86)?;
    assert!(demo.get_wall_hit());
    assert!((demo.get_position().x - 1.714).abs() < 0.05);
    let wall_hit = demo.get_impact();
    assert_eq!(wall_hit.body, demo.get_wall());
    assert!((wall_hit.position.x - 2.0).abs() < 0.01, "{wall_hit:?}");
    assert!(wall_hit.normal.x < -0.99, "{wall_hit:?}");
    assert_eq!(wall_hit.normal_impulse, 0.0);
    assert_eq!(
        interpreted.get("impact")?,
        Value::PhysicsContact3D(wall_hit)
    );
    render()?;

    demo.invoke_setup(1);
    interpreted.invoke("setup", &[Value::Int(1)])?;
    advance(&mut demo, 41)?;
    let lifted = demo.get_position();
    assert!(lifted.y > 1.45 && lifted.x > -0.7, "{lifted:?}");
    assert_eq!(demo.get_support(), demo.get_platform());
    advance(&mut demo, 14)?;
    assert!((demo.get_position().x - lifted.x).abs() < 1e-5);
    assert!((demo.get_position().y - lifted.y).abs() < 0.015);
    render()?;

    demo.invoke_setup(2);
    interpreted.invoke("setup", &[Value::Int(2)])?;
    advance(&mut demo, 94)?;
    assert!(
        demo.get_crate_position().x > 0.6,
        "{:?}",
        demo.get_crate_position()
    );
    assert_eq!(demo.get_entries(), 1);
    assert_eq!(interpreted.get("entries")?, Value::Int(1));
    let push_hit = demo.get_impact();
    assert_eq!(push_hit.body, demo.get_crate_body());
    assert!(push_hit.normal_impulse > 0.0, "{push_hit:?}");
    assert_eq!(
        interpreted.get("impact")?,
        Value::PhysicsContact3D(push_hit)
    );
    let pushed = demo.get_crate_position();
    let position = demo.get_position();
    render()?;

    demo.invoke_enable_hit_reports(false);
    interpreted.invoke("enable-hit-reports", &[Value::Bool(false)])?;
    demo.invoke_setup(2);
    interpreted.invoke("setup", &[Value::Int(2)])?;
    advance(&mut demo, 94)?;
    assert_eq!(demo.get_crate_position(), pushed);
    assert_eq!(demo.get_position(), position);
    assert_eq!(demo.get_hit_count(), 0);
    assert!(demo.get_step_hits().is_empty());
    assert_eq!(interpreted.get("hit-count")?, Value::Int(0));
    render()?;
    Ok(())
}

#[test]
fn physics_ray_queries_are_reactive_and_match_between_backends() -> TestResult {
    use physics_ray_cast_ui::PhysicsRayDemo;
    use xrack_runtime::PhysicsHit3D;

    const NAME: &str = "examples/puzzle3d/physics_ray_cast.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("PhysicsRayDemo")?;
    let mut demo = PhysicsRayDemo::new();

    let same = |demo: &PhysicsRayDemo| -> TestResult {
        let nearest = demo.get_nearest();
        assert_eq!(interpreted.get("nearest")?, Value::PhysicsHit3D(nearest));
        assert_eq!(
            interpreted.get("second")?,
            Value::PhysicsHit3D(demo.get_second())
        );
        assert_eq!(
            interpreted.get("hit-count")?,
            Value::Int(demo.get_hit_count())
        );
        assert_eq!(
            interpreted
                .physics_hit3d_model("hits")?
                .snapshot_untracked(),
            demo.get_hits()
        );
        assert_eq!(
            interpreted
                .physics_hit3d_model("nearest-list")?
                .snapshot_untracked(),
            demo.get_nearest_list(),
        );
        Ok(())
    };

    same(&demo)?;
    assert_eq!(demo.get_nearest(), PhysicsHit3D::default());
    assert_eq!(demo.get_hit_count(), 0);
    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    same(&demo)?;
    assert_eq!(demo.get_hit_count(), 2);
    let first = demo.get_nearest();
    assert_eq!(first.body, demo.get_moving_body());
    assert_eq!(demo.get_second().body, demo.get_back_body());
    assert!((first.distance - 2.65).abs() < 1e-5, "{first:?}");
    assert_eq!(first.normal, xrack_runtime::Vector3D::new(-1.0, 0.0, 0.0));

    demo.set_include_sensors(true);
    interpreted.set("include-sensors", Value::Bool(true))?;
    same(&demo)?;
    assert_eq!(demo.get_hit_count(), 3);
    let sensor = demo.get_nearest();
    assert_eq!(sensor.body, demo.get_sensor_body());
    assert!(sensor.sensor);
    assert_eq!(demo.get_second(), first);

    demo.set_group_only(true);
    interpreted.set("group-only", Value::Bool(true))?;
    same(&demo)?;
    assert_eq!(demo.get_nearest().body, demo.get_back_body());
    demo.invoke_make_front_group_visible();
    interpreted.invoke("make-front-group-visible", &[])?;
    same(&demo)?;
    assert_eq!(demo.get_nearest(), first);
    demo.set_skip_moving(true);
    interpreted.set("skip-moving", Value::Bool(true))?;
    same(&demo)?;
    assert_eq!(demo.get_nearest().body, demo.get_back_body());
    demo.set_skip_moving(false);
    interpreted.set("skip-moving", Value::Bool(false))?;

    for _ in 0..2 {
        demo.invoke_advance();
        interpreted.invoke("advance", &[])?;
        same(&demo)?;
    }
    assert_eq!(demo.get_hit_count(), 1);
    assert_eq!(demo.get_nearest().body, demo.get_back_body());

    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted));
    assert_eq!(
        aot_view.render(&mut renderer)?,
        interpreter_view.render(&mut renderer)?
    );
    Ok(())
}

#[test]
fn physics_shape_queries_match_in_aot_and_interpreter() -> TestResult {
    use physics_ray_cast_ui::PhysicsRayDemo;
    use xrack_runtime::PhysicsHit3D;

    const NAME: &str = "examples/puzzle3d/physics_ray_cast.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("PhysicsRayDemo")?;
    let mut demo = PhysicsRayDemo::new();
    let weak = demo.as_weak();
    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing PhysicsRayDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut same = |demo: &PhysicsRayDemo| -> TestResult {
        assert_eq!(
            interpreted.get("nearest")?,
            Value::PhysicsHit3D(demo.get_nearest())
        );
        assert_eq!(
            interpreted
                .physics_hit3d_model("hits")?
                .snapshot_untracked(),
            demo.get_hits()
        );
        assert_eq!(
            interpreted.int_model("overlaps")?.snapshot_untracked(),
            demo.get_overlaps()
        );
        assert_eq!(
            aot_view.render(&mut renderer)?,
            interpreter_view.render(&mut renderer)?
        );
        Ok(())
    };

    demo.set_mode(1);
    interpreted.set("mode", Value::Int(1))?;
    same(&demo)?;
    let sphere = demo.get_nearest();
    assert_eq!(sphere.body, demo.get_ledge_body());
    assert!((sphere.distance - 1.692).abs() < 0.003, "{sphere:?}");
    assert!(sphere.normal.y < -0.7);
    assert_eq!(demo.get_hits()[0], sphere);
    assert_eq!(demo.get_hits()[1].body, demo.get_moving_body());

    demo.set_mode(2);
    interpreted.set("mode", Value::Int(2))?;
    same(&demo)?;
    let compound = demo.get_nearest();
    assert_eq!(compound.body, demo.get_ledge_body());
    assert!((compound.distance - 1.525).abs() < 0.002, "{compound:?}");
    assert_eq!(
        demo.get_hits()
            .iter()
            .filter(|hit| hit.body == compound.body)
            .count(),
        1
    );

    demo.set_group_only(true);
    interpreted.set("group-only", Value::Bool(true))?;
    same(&demo)?;
    assert_eq!(demo.get_nearest().body, demo.get_back_body());
    demo.set_group_only(false);
    interpreted.set("group-only", Value::Bool(false))?;

    demo.set_mode(3);
    interpreted.set("mode", Value::Int(3))?;
    same(&demo)?;
    assert_eq!(demo.get_overlaps(), vec![demo.get_ledge_body()]);
    assert_eq!(demo.get_nearest(), PhysicsHit3D::default());
    demo.set_include_sensors(true);
    interpreted.set("include-sensors", Value::Bool(true))?;
    same(&demo)?;
    assert_eq!(
        demo.get_overlaps(),
        vec![demo.get_sensor_body(), demo.get_ledge_body()]
    );
    Ok(())
}

#[test]
fn kinematic_contact_modes_match_in_aot_and_interpreter() -> TestResult {
    use kinematic_reports_ui::KinematicReportsDemo;
    const NAME: &str = "examples/puzzle3d/kinematic_reports.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("KinematicReportsDemo")?;
    let demo = KinematicReportsDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing KinematicReportsDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    let mut previous_frame = None;
    for (fixed, pair) in [(false, false), (true, false), (false, true), (true, true)] {
        demo.invoke_configure(fixed, pair);
        interpreted.invoke("configure", &[Value::Bool(fixed), Value::Bool(pair)])?;
        for touching in [false, true] {
            demo.invoke_place(touching);
            interpreted.invoke("place", &[Value::Bool(touching)])?;
            assert_eq!(demo.get_report_fixed(), fixed);
            assert_eq!(demo.get_report_pair(), pair);
            assert_eq!(demo.get_fixed_count() > 0, fixed && touching);
            assert_eq!(demo.get_pair_count() > 0, pair && touching);
            assert_eq!(
                interpreted.get("blue-position")?,
                Value::Point3D(demo.get_blue_position())
            );
            assert_eq!(
                interpreted.get("teal-position")?,
                Value::Point3D(demo.get_teal_position())
            );
            assert_eq!(
                interpreted.get("blue-contact")?,
                Value::PhysicsContact3D(demo.get_blue_contact())
            );
            assert_eq!(
                interpreted.get("teal-contact")?,
                Value::PhysicsContact3D(demo.get_teal_contact())
            );
        }
        if fixed {
            let contact = demo.get_blue_contact();
            assert_eq!(contact.body, demo.get_fixed());
            assert!(contact.normal.x < -0.99);
            assert_eq!(contact.normal_impulse, 0.0);
        }
        if pair {
            let contact = demo.get_teal_contact();
            assert_eq!(contact.body, demo.get_parked());
            assert!(contact.normal.x < -0.99);
            assert_eq!(contact.normal_impulse, 0.0);
        }
        assert!((demo.get_blue_position().x + 0.28).abs() < 1e-5);
        let png = aot_view.render(&mut renderer)?.to_vec();
        assert_eq!(png, interpreted_view.render(&mut renderer)?);
        assert_ne!(previous_frame.as_ref(), Some(&png));
        previous_frame = Some(png);
    }
    Ok(())
}

#[test]
fn declarative_body_synchronization_matches_in_aot_and_interpreter() -> TestResult {
    use authored_physics_ui::AuthoredPhysicsDemo;
    use std::sync::Arc;
    use xrack_runtime::{
        Point3D, Vector3D,
        scene3d::{Ray3D, SceneModel, raycast},
    };

    fn model(view: &PreviewScene) -> TestResult<Arc<SceneModel>> {
        let (view, _) = inspect(view)?;
        let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
            return Err("missing projected model".into());
        };
        Ok(model.clone())
    }

    fn check_ball(model: &SceneModel, position: Point3D) -> TestResult<Point3D> {
        let center = model
            .map_position("ball", "", Point3D::default())
            .ok_or("missing ball transform")?;
        assert!(
            (center.x - position.x).abs() < 1e-6
                && (center.y - position.y).abs() < 1e-6
                && (center.z - position.z).abs() < 1e-6,
            "scene center {center:?}, physics center {position:?}"
        );
        let ray = Ray3D::new([center.x as f32, center.y as f32, 4.0], [0.0, 0.0, -1.0])
            .ok_or("invalid ball test ray")?;
        assert_eq!(
            raycast(model, ray, 6.0).map(|hit| hit.object_id),
            Some("ball")
        );
        model
            .map_position("marker", "", Point3D::default())
            .ok_or_else(|| "missing child transform".into())
    }
    const NAME: &str = "examples/puzzle3d/authored_physics.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("AuthoredPhysicsDemo")?;
    let demo = AuthoredPhysicsDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing AuthoredPhysicsDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    assert_eq!(demo.get_ball(), 0);
    assert_eq!(interpreted.get("ball")?, Value::Int(0));
    assert_eq!(
        aot_view.render(&mut renderer)?,
        interpreted_view.render(&mut renderer)?
    );
    let fallback = model(&aot_view)?;
    let fallback_marker = check_ball(&fallback, demo.get_ball_position())?;
    demo.invoke_freeze_view();
    interpreted.invoke("freeze-view", &[])?;
    demo.invoke_launch();
    interpreted.invoke("launch", &[])?;
    assert_eq!(
        aot_view.render(&mut renderer)?,
        interpreted_view.render(&mut renderer)?
    );
    assert!(Arc::ptr_eq(&model(&aot_view)?, &fallback));
    assert_ne!(demo.get_displayed_center(), demo.get_ball_position());
    demo.invoke_freeze_view();
    interpreted.invoke("freeze-view", &[])?;
    let ball = demo.get_ball();
    let wall = demo.get_wall();
    let position = demo.get_ball_position();
    let velocity = demo.get_ball_velocity();
    assert!(ball > 0 && wall > 0);
    assert!(position.x > -1.5 && velocity.x > 0.0);
    let launched = aot_view.render(&mut renderer)?.to_vec();
    assert_eq!(launched, interpreted_view.render(&mut renderer)?);
    let launched_model = model(&aot_view)?;
    let moved_marker = check_ball(&launched_model, position)?;
    assert_eq!(
        check_ball(&*model(&interpreted_view)?, position)?,
        moved_marker
    );
    assert_ne!(moved_marker, fallback_marker);
    let mut previous = launched_model;

    for (reverse, shift, duplicate) in [
        (true, false, false),
        (true, true, false),
        (false, false, true),
    ] {
        demo.invoke_commit(reverse, shift, duplicate);
        interpreted.invoke(
            "commit",
            &[
                Value::Bool(reverse),
                Value::Bool(shift),
                Value::Bool(duplicate),
            ],
        )?;
        assert_eq!(demo.get_accepted(), !duplicate);
        assert_eq!(interpreted.get("accepted")?, Value::Bool(!duplicate));
        assert_eq!(demo.get_ball(), ball);
        assert_eq!(interpreted.get("ball")?, Value::Int(ball));
        assert_eq!(demo.get_ball_position(), position);
        assert_eq!(interpreted.get("ball-position")?, Value::Point3D(position));
        assert_eq!(demo.get_ball_velocity(), velocity);
        assert_eq!(interpreted.get("ball-velocity")?, Value::Vector3D(velocity));
        assert_eq!(interpreted.get("wall")?, Value::Int(demo.get_wall()));
        assert_eq!(wall == demo.get_wall(), !shift && !duplicate);
        let frame = aot_view.render(&mut renderer)?.to_vec();
        assert_eq!(frame, interpreted_view.render(&mut renderer)?);
        assert_eq!(frame == launched, !shift && !duplicate);
        let current = model(&aot_view)?;
        assert_eq!(check_ball(&current, position)?, moved_marker);
        let wall_pose = current
            .map_position("wall", "", Point3D::default())
            .ok_or("missing wall transform")?;
        assert!((wall_pose.x - if shift || duplicate { 2.8 } else { 1.85 }).abs() < 1e-6);
        assert_eq!(wall_pose.z, 0.0);
        let local_forward = Vector3D::new(0.0, 0.0, 1.0);
        assert_eq!(
            current.map_direction("wall", "", local_forward),
            Some(local_forward)
        );
        assert_eq!(Arc::ptr_eq(&previous, &current), !shift || duplicate);
        previous = current;
    }
    demo.invoke_advance();
    interpreted.invoke("advance", &[])?;
    assert!(demo.get_ball_position().x > position.x);
    assert_eq!(
        interpreted.get("ball-position")?,
        Value::Point3D(demo.get_ball_position()),
    );
    assert_eq!(
        aot_view.render(&mut renderer)?,
        interpreted_view.render(&mut renderer)?
    );
    let advanced = model(&aot_view)?;
    assert_eq!(demo.get_position_before_step(), position);
    assert_eq!(
        interpreted.get("position-before-step")?,
        Value::Point3D(position)
    );
    assert_ne!(
        check_ball(&advanced, demo.get_ball_position())?,
        moved_marker
    );
    assert!(!Arc::ptr_eq(&previous, &advanced));

    demo.invoke_set_smoothing(true);
    interpreted.invoke("set-smoothing", &[Value::Bool(true)])?;
    demo.invoke_advance();
    interpreted.invoke("advance", &[])?;
    let smoothed = demo.get_smoothed_position();
    assert_ne!(demo.get_ball_position(), smoothed);
    assert_eq!(
        interpreted.get("smoothed-position")?,
        Value::Point3D(smoothed)
    );
    assert_eq!(
        aot_view.render(&mut renderer)?,
        interpreted_view.render(&mut renderer)?
    );
    let displayed = model(&aot_view)?;
    check_ball(&displayed, smoothed)?;
    assert!(Arc::ptr_eq(&displayed, &demo.get_displayed_scene()));
    let Value::SceneObjects(interpreted_scene) = interpreted.get("displayed-scene")? else {
        return Err("expected a public scene snapshot from the interpreter".into());
    };
    assert!(Arc::ptr_eq(&interpreted_scene, &model(&interpreted_view)?));
    assert_eq!(demo.get_displayed_center(), smoothed);
    assert_eq!(demo.get_displayed_hit().object_id, "ball");
    assert_eq!(
        interpreted.get("displayed-hit")?,
        Value::Hit3D(demo.get_displayed_hit())
    );

    demo.invoke_freeze_view();
    interpreted.invoke("freeze-view", &[])?;
    let frozen = aot_view.render(&mut renderer)?.to_vec();
    demo.invoke_advance();
    interpreted.invoke("advance", &[])?;
    assert_ne!(demo.get_smoothed_position(), smoothed);
    assert_eq!(aot_view.render(&mut renderer)?, frozen);
    assert_eq!(interpreted_view.render(&mut renderer)?, frozen);
    assert!(Arc::ptr_eq(&model(&aot_view)?, &displayed));
    assert_eq!(demo.get_displayed_center(), smoothed);
    assert_eq!(
        interpreted.get("displayed-center")?,
        Value::Point3D(smoothed)
    );
    assert_eq!(demo.get_displayed_hit().object_id, "ball");

    demo.invoke_freeze_view();
    interpreted.invoke("freeze-view", &[])?;
    let current = aot_view.render(&mut renderer)?;
    assert_ne!(current, frozen);
    assert_eq!(current, interpreted_view.render(&mut renderer)?);
    assert_eq!(demo.get_displayed_center(), demo.get_smoothed_position());
    Ok(())
}

#[test]
fn ccd_and_world_units_match_in_aot_and_interpreter() -> TestResult {
    use physics_ccd_ui::PhysicsCcdDemo;
    const NAME: &str = "examples/puzzle3d/physics_ccd.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("PhysicsCcdDemo")?;
    let demo = PhysicsCcdDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing PhysicsCcdDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut last = None;
    for (speculative, dynamic_wall, units) in [
        (false, false, 1.0),
        (false, true, 1.0),
        (true, false, 1.0),
        (false, false, 100.0),
    ] {
        demo.set_speculative(speculative);
        demo.set_dynamic_wall(dynamic_wall);
        demo.set_units(units);
        interpreted.set("speculative", Value::Bool(speculative))?;
        interpreted.set("dynamic-wall", Value::Bool(dynamic_wall))?;
        interpreted.set("units", Value::Float(units))?;
        demo.invoke_shoot();
        interpreted.invoke("shoot", &[])?;
        let blue = demo.get_blue_position();
        let gold = demo.get_gold_position();
        assert!(blue.x > 1.5 && gold.x < 0.0, "{blue:?} {gold:?}");
        assert_eq!(demo.get_solver_length(), units);
        assert_eq!(demo.get_gold_full(), !speculative);
        assert_eq!(demo.get_budget(), if speculative { 0 } else { 4 });
        assert_eq!(demo.get_horizon(), if speculative { 4.0 } else { 0.0 });
        for (name, value) in [
            ("blue-position", Value::Point3D(blue)),
            ("gold-position", Value::Point3D(gold)),
            ("solver-length", Value::Float(units)),
            ("budget", Value::Int(demo.get_budget())),
            ("horizon", Value::Float(demo.get_horizon())),
            ("gold-full", Value::Bool(demo.get_gold_full())),
        ] {
            assert_eq!(interpreted.get(name)?, value, "{name}");
        }
        let image = aot_view.render(&mut renderer)?.to_vec();
        assert_eq!(image, interpreted_view.render(&mut renderer)?);
        assert_ne!(last.as_ref(), Some(&image));
        last = Some(image);
    }
    Ok(())
}

#[test]
fn physics_debug_lines_match_in_aot_and_interpreter_with_retained_views() -> TestResult {
    use physics_debug_ui::PhysicsDebugDemo;
    const NAME: &str = "examples/puzzle3d/physics_debug.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("PhysicsDebugDemo")?;
    let demo = PhysicsDebugDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing PhysicsDebugDemo")?;
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;
    let mut frame = |gpu: &mut Renderer, preview_gpu: &mut Renderer| -> TestResult<Vec<u8>> {
        let actual = aot_view.render(gpu)?.to_vec();
        assert_eq!(actual, interpreted_view.render(preview_gpu)?);
        Ok(actual)
    };

    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    assert!(demo.get_compound() > 0 && demo.get_platform() > 0 && demo.get_surface() > 0);
    let initial = frame(&mut gpu, &mut preview_gpu)?;
    let whole = gpu.scene_draw_stats();
    assert_eq!(whole.physics_debug_draws, 1);
    assert_eq!(whole.physics_debug_uploads, 1);
    assert!(whole.physics_debug_lines > 70);
    assert_eq!(
        whole.physics_debug_lines,
        preview_gpu.scene_draw_stats().physics_debug_lines
    );

    for _ in 0..3 {
        assert_eq!(frame(&mut gpu, &mut preview_gpu)?, initial);
    }
    assert_eq!(gpu.scene_draw_stats().physics_debug_uploads, 1);
    assert!(gpu.scene_draw_stats().viewport_cache_hits > whole.viewport_cache_hits);

    demo.invoke_move_colliders();
    interpreted.invoke("move-colliders", &[])?;
    let moved = frame(&mut gpu, &mut preview_gpu)?;
    assert_ne!(moved, initial);
    let changed = gpu.scene_draw_stats();
    assert_eq!(changed.physics_debug_uploads, 2);
    assert_eq!(
        changed.physics_debug_lines - whole.physics_debug_lines,
        whole.physics_debug_lines
    );

    demo.set_focus(true);
    interpreted.set("focus", Value::Bool(true))?;
    let focused_image = frame(&mut gpu, &mut preview_gpu)?;
    assert_ne!(focused_image, moved);
    let focused = gpu.scene_draw_stats();
    let local_lines = focused.physics_debug_lines - changed.physics_debug_lines;
    assert!(local_lines > 12 && local_lines < whole.physics_debug_lines);
    assert_eq!(focused.physics_debug_uploads, 3);

    demo.set_show_model(false);
    interpreted.set("show-model", Value::Bool(false))?;
    let debug_only = frame(&mut gpu, &mut preview_gpu)?;
    let no_meshes = gpu.scene_draw_stats();
    assert_eq!(no_meshes.color_draws, focused.color_draws);
    assert_eq!(
        no_meshes.physics_debug_draws,
        focused.physics_debug_draws + 1
    );
    assert_eq!(
        no_meshes.physics_debug_uploads,
        focused.physics_debug_uploads
    );

    demo.set_debug(false);
    interpreted.set("debug", Value::Bool(false))?;
    assert_ne!(frame(&mut gpu, &mut preview_gpu)?, debug_only);
    let disabled = gpu.scene_draw_stats();
    assert_eq!(disabled.physics_debug_draws, no_meshes.physics_debug_draws);
    assert_eq!(
        disabled.physics_debug_uploads,
        no_meshes.physics_debug_uploads
    );
    Ok(())
}

#[test]
fn physics_clock_batches_and_interpolates_in_both_backends() -> TestResult {
    use physics_clock_ui::PhysicsClockDemo;
    use std::time::{Duration, Instant};
    const NAME: &str = "examples/puzzle3d/physics_clock.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("PhysicsClockDemo")?;
    let demo = PhysicsClockDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing PhysicsClockDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let exact = |demo: &PhysicsClockDemo| -> TestResult {
        for (name, value) in [
            ("steps", Value::Int(demo.get_steps())),
            ("rendered-frames", Value::Int(demo.get_rendered_frames())),
            ("frame-delta", Value::Float(demo.get_frame_delta())),
            ("frame-elapsed", Value::Float(demo.get_frame_elapsed())),
            ("entered", Value::Int(demo.get_entered())),
            ("exited", Value::Int(demo.get_exited())),
            ("world-running", Value::Bool(demo.get_world_running())),
            ("scale", Value::Float(demo.get_scale())),
            ("simulated", Value::Float(demo.get_simulated())),
            ("dropped", Value::Float(demo.get_dropped())),
            ("alpha", Value::Float(demo.get_alpha())),
            ("actual", Value::Point3D(demo.get_actual())),
            ("drawn", Value::Point3D(demo.get_drawn())),
            ("yaw-pitch-roll", Value::Vector3D(demo.get_yaw_pitch_roll())),
        ] {
            assert_eq!(interpreted.get(name)?, value, "{name}");
        }
        Ok(())
    };
    let advance = |demo: &mut PhysicsClockDemo, time, limit| -> TestResult {
        demo.invoke_frame(time, limit);
        interpreted.invoke("frame", &[Value::Float(time), Value::Int(limit)])?;
        exact(demo)
    };
    let mut render_pair = || -> TestResult {
        assert_eq!(
            aot_view.render(&mut renderer)?,
            interpreted_view.render(&mut renderer)?
        );
        Ok(())
    };
    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    for elapsed in [0.041, 0.05, 0.029] {
        advance(&mut demo, elapsed, 8)?;
    }
    assert_eq!(demo.get_steps(), 1);
    assert_eq!(demo.get_alpha(), 0.0);
    assert_eq!(demo.get_drawn().x, -2.0);
    let committed = demo.get_actual().x;
    advance(&mut demo, 0.06, 8)?;
    assert_eq!(demo.get_steps(), 0);
    assert_eq!(demo.get_actual().x, committed);
    assert!((demo.get_drawn().x - (committed - 2.0) / 2.0).abs() < 1e-6);
    assert!((demo.get_yaw_pitch_roll().x - demo.get_actual_yaw().x * 0.5).abs() < 1e-5);
    render_pair()?;

    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    advance(&mut demo, 1.37, 8)?;
    assert_eq!(
        (demo.get_steps(), demo.get_entered(), demo.get_exited()),
        (8, 1, 1)
    );
    assert!((demo.get_dropped() - 0.36).abs() < 1e-12);
    assert!((demo.get_simulated() - 0.96).abs() < 1e-12);
    render_pair()?;

    demo.invoke_pause(false);
    interpreted.invoke("pause", &[Value::Bool(false)])?;
    let paused = demo.get_drawn();
    advance(&mut demo, 10.0, 8)?;
    assert_eq!(demo.get_steps(), 0);
    assert_eq!(demo.get_drawn(), paused);
    demo.invoke_manual();
    interpreted.invoke("manual", &[])?;
    exact(&demo)?;
    assert_eq!((demo.get_entered(), demo.get_exited()), (1, 1));
    assert_eq!(demo.get_drawn(), demo.get_actual());
    render_pair()?;

    demo.invoke_pause(true);
    interpreted.invoke("pause", &[Value::Bool(true)])?;
    demo.invoke_slow(0.5);
    interpreted.invoke("slow", &[Value::Float(0.5)])?;
    advance(&mut demo, 0.24, 8)?;
    assert_eq!(demo.get_steps(), 1);

    demo.invoke_start_realtime();
    interpreted.invoke("start-realtime", &[])?;
    render_pair()?;
    assert_eq!(demo.get_rendered_frames(), 0);
    let now = Instant::now();
    for (millis, frame, delta, steps) in [
        (0, 1, 0.0, 0),
        (40, 2, 0.04, 0),
        (120, 3, 0.08, 1),
        (120, 3, 0.08, 1),
        (180, 4, 0.06, 0),
    ] {
        let at = now + Duration::from_millis(millis);
        aot_view.advance_at(&mut renderer, at);
        interpreted_view.advance_at(&mut renderer, at);
        exact(&demo)?;
        assert_eq!(demo.get_rendered_frames(), frame);
        assert_eq!(demo.get_frame_delta(), delta);
        assert!(
            (demo.get_frame_elapsed() - Duration::from_millis(millis).as_secs_f64()).abs() < 1e-12
        );
        assert_eq!(demo.get_steps(), steps);
        assert_eq!(
            aot_view.draw_prepared(&mut renderer)?,
            interpreted_view.draw_prepared(&mut renderer)?
        );
    }
    assert!(aot_view.next_frame_delay().is_some());
    assert!(interpreted_view.next_frame_delay().is_some());
    demo.invoke_pause(false);
    interpreted.invoke("pause", &[Value::Bool(false)])?;
    let paused = now + Duration::from_secs(3);
    aot_view.advance_at(&mut renderer, paused);
    interpreted_view.advance_at(&mut renderer, paused);
    exact(&demo)?;
    assert_eq!(demo.get_rendered_frames(), 4);
    assert_eq!(demo.get_simulated(), 0.12);
    demo.invoke_pause(true);
    interpreted.invoke("pause", &[Value::Bool(true)])?;
    let resumed = paused + Duration::from_secs(3);
    aot_view.advance_at(&mut renderer, resumed);
    interpreted_view.advance_at(&mut renderer, resumed);
    exact(&demo)?;
    assert_eq!(demo.get_rendered_frames(), 1);
    assert_eq!(demo.get_frame_delta(), 0.0);
    assert_eq!(demo.get_simulated(), 0.12);
    Ok(())
}

#[test]
fn physics_kinematic_handoff_matches_in_aot_and_interpreter() -> TestResult {
    use physics_kinematic_ui::PhysicsKinematicDemo;
    use xrack_runtime::Vector3D;

    const NAME: &str = "examples/puzzle3d/physics_kinematic.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("PhysicsKinematicDemo")?;
    let demo = PhysicsKinematicDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing PhysicsKinematicDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let exact = |demo: &PhysicsKinematicDemo| -> TestResult {
        for (name, value) in [
            ("arm", Value::Int(demo.get_arm())),
            ("entries", Value::Int(demo.get_entries())),
            ("controlled", Value::Bool(demo.get_controlled())),
            ("mass", Value::Float(demo.get_mass())),
            ("position", Value::Point3D(demo.get_position())),
            ("rotation", Value::Vector3D(demo.get_rotation())),
            ("speed", Value::Vector3D(demo.get_speed())),
            ("spin", Value::Vector3D(demo.get_spin())),
        ] {
            assert_eq!(interpreted.get(name)?, value, "{name}");
        }
        Ok(())
    };
    let mut render_pair = || -> TestResult {
        assert_eq!(
            aot_view.render(&mut renderer)?,
            interpreted_view.render(&mut renderer)?
        );
        Ok(())
    };

    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    let handle = demo.get_arm();
    let start = demo.get_position();
    assert!(handle > 0 && !demo.get_controlled());
    demo.invoke_take();
    interpreted.invoke("take", &[])?;
    assert!(demo.get_controlled());
    assert_eq!(demo.get_speed(), Vector3D::default());
    for _ in 0..4 {
        let prior = demo.get_position();
        demo.invoke_queue_next();
        interpreted.invoke("queue-next", &[])?;
        assert_eq!(demo.get_position(), prior);
        exact(&demo)?;
        demo.invoke_advance();
        interpreted.invoke("advance", &[])?;
        exact(&demo)?;
    }
    let held = demo.get_position();
    let rotation = demo.get_rotation();
    let linear = demo.get_speed();
    let angular = demo.get_spin();
    assert!((rotation.x - 32.0).abs() < 1e-4);
    assert!((angular.y - 8.0_f64.to_radians() / 0.05).abs() < 1e-5);
    assert_eq!(held.y, start.y);
    assert_eq!(demo.get_entries(), 1);
    render_pair()?;

    demo.invoke_queue_next();
    interpreted.invoke("queue-next", &[])?;
    demo.invoke_release();
    interpreted.invoke("release", &[])?;
    exact(&demo)?;
    assert!(!demo.get_controlled());
    assert_eq!(demo.get_arm(), handle);
    assert_eq!(demo.get_mass(), 2.5);
    assert_eq!(demo.get_position(), held);
    assert_eq!(demo.get_rotation(), rotation);
    assert_eq!(demo.get_speed(), linear);
    assert_eq!(demo.get_spin(), angular);
    for _ in 0..4 {
        demo.invoke_advance();
        interpreted.invoke("advance", &[])?;
        exact(&demo)?;
    }
    assert!(demo.get_position().y < held.y - 0.04);
    assert!(demo.get_rotation().x > 50.0);
    assert_eq!(demo.get_entries(), 1);
    render_pair()?;
    Ok(())
}

#[test]
fn physics_mass_modes_match_in_aot_and_interpreter() -> TestResult {
    use physics_mass_ui::PhysicsMassDemo;
    use xrack_runtime::Vector3D;

    const NAME: &str = "examples/puzzle3d/physics_mass.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("PhysicsMassDemo")?;
    let demo = PhysicsMassDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing PhysicsMassDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let exact = |demo: &PhysicsMassDemo| -> TestResult {
        for (name, value) in [
            ("density", Value::Float(demo.get_density())),
            ("amber-mass", Value::Float(demo.get_amber_mass())),
            ("teal-mass", Value::Float(demo.get_teal_mass())),
            ("amber-center", Value::Point3D(demo.get_amber_center())),
            (
                "amber-local-center",
                Value::Point3D(demo.get_amber_local_center()),
            ),
            ("amber-inertia", Value::Vector3D(demo.get_amber_inertia())),
            ("amber-column", Value::Vector3D(demo.get_amber_column())),
            ("amber-spin", Value::Vector3D(demo.get_amber_spin())),
            ("teal-spin", Value::Vector3D(demo.get_teal_spin())),
            ("amber-speed", Value::Vector3D(demo.get_amber_speed())),
            ("teal-speed", Value::Vector3D(demo.get_teal_speed())),
            ("amber-position", Value::Point3D(demo.get_amber_position())),
            ("teal-position", Value::Point3D(demo.get_teal_position())),
        ] {
            assert_eq!(interpreted.get(name)?, value, "{name}");
        }
        Ok(())
    };
    let kick = |demo: &mut PhysicsMassDemo| -> TestResult {
        demo.invoke_kick();
        interpreted.invoke("kick", &[])?;
        exact(demo)
    };
    let mut render_after_steps = |demo: &mut PhysicsMassDemo| -> TestResult {
        for _ in 0..15 {
            demo.invoke_advance();
            interpreted.invoke("advance", &[])?;
            exact(demo)?;
        }
        assert_eq!(
            aot_view.render(&mut renderer)?,
            interpreted_view.render(&mut renderer)?
        );
        Ok(())
    };

    demo.invoke_reset(0);
    interpreted.invoke("reset", &[Value::Int(0)])?;
    assert_eq!((demo.get_amber_mass(), demo.get_teal_mass()), (1.0, 4.0));
    kick(&mut demo)?;
    assert_eq!(
        (demo.get_amber_speed().x, demo.get_teal_speed().x),
        (2.0, 0.5)
    );
    demo.invoke_toggle_density();
    interpreted.invoke("toggle-density", &[])?;
    assert_eq!((demo.get_amber_mass(), demo.get_teal_mass()), (2.0, 4.0));
    kick(&mut demo)?;
    assert_eq!(
        (demo.get_amber_speed().x, demo.get_teal_speed().x),
        (3.0, 1.0)
    );
    render_after_steps(&mut demo)?;

    demo.invoke_reset(1);
    interpreted.invoke("reset", &[Value::Int(1)])?;
    assert!((demo.get_amber_local_center().y - 0.65).abs() < 1e-6);
    kick(&mut demo)?;
    assert!((demo.get_amber_spin().z - 1.625).abs() < 1e-5);
    assert_eq!(demo.get_teal_spin(), Vector3D::default());
    render_after_steps(&mut demo)?;

    demo.invoke_reset(2);
    interpreted.invoke("reset", &[Value::Int(2)])?;
    assert!((demo.get_amber_column().y - 0.75).abs() < 1e-5);
    kick(&mut demo)?;
    assert!((demo.get_amber_spin().y + 1.5 / 2.4375).abs() < 1e-5);
    assert_eq!(demo.get_teal_spin(), Vector3D::new(1.0, 0.0, 0.0));
    render_after_steps(&mut demo)?;
    Ok(())
}

#[test]
fn physics_material_changes_match_in_aot_and_interpreter() -> TestResult {
    use physics_materials_ui::PhysicsMaterialsDemo;

    const NAME: &str = "examples/puzzle3d/physics_materials.xr";
    let source = std::fs::read_to_string(NAME)?;
    let module = xrack_compiler::prepare_preview(&source, NAME, Default::default())?.module;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module, NAME, None)?;
    let interpreted = program.create("PhysicsMaterialsDemo")?;
    let demo = PhysicsMaterialsDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreted_view = PreviewScene::new(Box::new(interpreted.clone()));
    let mut demo = weak.upgrade().ok_or("missing PhysicsMaterialsDemo")?;
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let exact = |demo: &PhysicsMaterialsDemo| -> TestResult {
        for (name, value) in [
            ("amber-position", Value::Point3D(demo.get_amber_position())),
            ("teal-position", Value::Point3D(demo.get_teal_position())),
            ("grip", Value::Float(demo.get_grip())),
            ("slide", Value::Float(demo.get_slide())),
            ("elasticity", Value::Float(demo.get_elasticity())),
            ("peak", Value::Float(demo.get_peak())),
        ] {
            assert_eq!(interpreted.get(name)?, value, "{name}");
        }
        Ok(())
    };
    let advance = |demo: &mut PhysicsMaterialsDemo, count| -> TestResult {
        for _ in 0..count {
            demo.invoke_advance();
            interpreted.invoke("advance", &[])?;
            exact(demo)?;
        }
        Ok(())
    };
    let mut render_pair = || -> TestResult {
        assert_eq!(
            aot_view.render(&mut renderer)?,
            interpreted_view.render(&mut renderer)?
        );
        Ok(())
    };

    demo.invoke_reset(0);
    interpreted.invoke("reset", &[Value::Int(0)])?;
    demo.invoke_push(3.0);
    interpreted.invoke("push", &[Value::Float(3.0)])?;
    advance(&mut demo, 36)?;
    assert!((demo.get_amber_position().x + 2.0).abs() < 0.003);
    assert!(demo.get_teal_position().x > -1.7);
    render_pair()?;

    demo.invoke_change_grip();
    interpreted.invoke("change-grip", &[])?;
    exact(&demo)?;
    assert_eq!(demo.get_grip(), demo.get_slide());
    advance(&mut demo, 20)?;
    assert!(demo.get_amber_position().x > -1.89);
    render_pair()?;

    demo.invoke_reset(0);
    interpreted.invoke("reset", &[Value::Int(0)])?;
    demo.invoke_push(14.0);
    interpreted.invoke("push", &[Value::Float(14.0)])?;
    advance(&mut demo, 20)?;
    assert!(demo.get_amber_speed().x > 3.5);

    demo.invoke_reset(1);
    interpreted.invoke("reset", &[Value::Int(1)])?;
    advance(&mut demo, 57)?;
    assert!(demo.get_peak() > 0.35);
    assert!(demo.get_amber_position().y > demo.get_teal_position().y + 0.25);
    render_pair()?;
    Ok(())
}

#[test]
fn scripted_dynamic_body_controls_match_in_aot_and_interpreter() -> TestResult {
    use body_dynamics_ui::BodyDynamicsDemo;

    const NAME: &str = "examples/puzzle3d/body_dynamics.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("BodyDynamicsDemo")?;
    let demo = BodyDynamicsDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut aot_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);
    let mut demo = weak.upgrade().ok_or("missing dynamics demo")?;
    let advance = |demo: &mut BodyDynamicsDemo| -> TestResult {
        demo.invoke_advance();
        interpreted.invoke("advance", &[])?;
        assert_eq!(
            interpreted.get("position")?,
            Value::Point3D(demo.get_position())
        );
        assert_eq!(interpreted.get("speed")?, Value::Vector3D(demo.get_speed()));
        assert_eq!(interpreted.get("spin")?, Value::Vector3D(demo.get_spin()));
        Ok(())
    };
    let mut render = || -> TestResult {
        assert_eq!(
            aot_view.render(&mut aot_gpu)?,
            interpreter_view.render(&mut interpreter_gpu)?
        );
        Ok(())
    };
    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    let body = demo.get_body();
    let start = demo.get_position();
    let zero = xrack_runtime::Vector3D::default();
    assert!(body > 0);
    demo.invoke_push_once();
    interpreted.invoke("push-once", &[])?;
    assert_eq!(demo.get_speed(), zero);
    advance(&mut demo)?;
    let one_step = demo.get_speed().x;
    assert!((one_step - 0.096).abs() < 1e-5, "{one_step}");
    advance(&mut demo)?;
    assert_eq!(demo.get_speed().x, one_step);
    demo.invoke_drive(true);
    interpreted.invoke("drive", &[Value::Bool(true)])?;
    for _ in 0..12 {
        advance(&mut demo)?;
    }
    assert!((demo.get_speed().x - one_step - 0.576).abs() < 1e-5);
    render()?;

    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    assert_eq!(demo.get_body(), body);
    assert_eq!(demo.get_position(), start);
    assert_eq!(demo.get_speed(), zero);
    demo.invoke_kick();
    interpreted.invoke("kick", &[])?;
    assert_eq!(demo.get_speed().x, 2.0);
    assert!(demo.get_spin().z < -1.0);
    for _ in 0..8 {
        advance(&mut demo)?;
    }
    assert!(demo.get_orientation().z.abs() > 5.0);
    render()?;

    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    demo.invoke_lock(true);
    interpreted.invoke("lock", &[Value::Bool(true)])?;
    demo.invoke_kick();
    interpreted.invoke("kick", &[])?;
    assert_eq!(demo.get_spin(), zero);
    assert_eq!(demo.get_speed().x, 2.0);
    demo.invoke_enable_gravity(true);
    interpreted.invoke("enable-gravity", &[Value::Bool(true)])?;
    for _ in 0..20 {
        advance(&mut demo)?;
    }
    assert!(demo.get_position().y < 0.6);
    assert_eq!(demo.get_orientation(), zero);
    render()?;
    Ok(())
}

#[test]
fn body_contact_reports_have_matching_points_and_behavior_in_both_backends() -> TestResult {
    use body_contacts_ui::BodyContactsDemo;

    const NAME: &str = "examples/puzzle3d/body_contacts.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("BodyContactsDemo")?;
    let demo = BodyContactsDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut aot_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);
    let mut demo = weak.upgrade().ok_or("missing body contact demo")?;
    let mut frame = || -> TestResult<Vec<u8>> {
        let png = aot_view.render(&mut aot_gpu)?.to_vec();
        assert_eq!(
            png.as_slice(),
            interpreter_view.render(&mut interpreter_gpu)?
        );
        Ok(png)
    };

    demo.invoke_launch();
    interpreted.invoke("launch", &[])?;
    let before = frame()?;
    let mut saw_contact = false;
    for _ in 0..29 {
        demo.invoke_advance();
        interpreted.invoke("advance", &[])?;
        let contacts = demo.get_step_contacts();
        let Value::PhysicsContact3DList(interpreted_contacts) = interpreted.get("step-contacts")?
        else {
            return Err("expected a typed contact list".into());
        };
        assert_eq!(contacts, interpreted_contacts.snapshot_untracked());
        saw_contact |= !contacts.is_empty();
    }
    assert!(saw_contact && demo.get_reported() > 0);
    assert_eq!((demo.get_entered(), demo.get_exited()), (1, 1));
    assert_eq!(interpreted.get("entered")?, Value::Int(1));
    assert_eq!(interpreted.get("exited")?, Value::Int(1));
    let impact = demo.get_impact();
    assert_eq!(impact.body, demo.get_wall());
    assert!(
        impact.position.x > 0.0 && impact.position.x < 0.25,
        "{impact:?}"
    );
    assert!(impact.normal.x < -0.99 && impact.impulse.x < -9.0);
    assert_eq!(interpreted.get("impact")?, Value::PhysicsContact3D(impact));
    assert!(demo.get_step_contacts().is_empty());
    let rebound = demo.get_position();
    assert!(rebound.x < -0.3);
    assert_ne!(before, frame()?);

    demo.invoke_enable_reports(false);
    interpreted.invoke("enable-reports", &[Value::Bool(false)])?;
    demo.invoke_launch();
    interpreted.invoke("launch", &[])?;
    for _ in 0..29 {
        demo.invoke_advance();
        interpreted.invoke("advance", &[])?;
    }
    assert_eq!(demo.get_reported(), 0);
    assert_eq!(interpreted.get("reported")?, Value::Int(0));
    assert_eq!(demo.get_impact().body, 0);
    assert_eq!(demo.get_position(), rebound);
    assert_eq!(interpreted.get("position")?, Value::Point3D(rebound));
    assert_eq!((demo.get_entered(), demo.get_exited()), (1, 1));
    let _ = frame()?;

    demo.invoke_launch();
    interpreted.invoke("launch", &[])?;
    demo.invoke_advance_batch(0.64, 40);
    interpreted.invoke("advance-batch", &[Value::Float(0.64), Value::Int(40)])?;
    let wall = demo.get_wall();
    assert_eq!(demo.get_ticks(), 40);
    assert_eq!(demo.get_step_entered(), [wall]);
    assert_eq!(demo.get_step_exited(), [wall]);
    assert_eq!(interpreted.get("step-entered")?, Value::IntList(vec![wall]));
    assert_eq!(interpreted.get("step-exited")?, Value::IntList(vec![wall]));
    assert!(demo.get_touching().is_empty());
    let _ = frame()?;
    Ok(())
}

#[test]
fn asset_colliders_share_rest_geometry_and_height_samples_in_both_backends() -> TestResult {
    use mesh_colliders_ui::MeshCollidersDemo;

    const NAME: &str = "examples/puzzle3d/mesh_colliders.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("MeshCollidersDemo")?;
    let demo = MeshCollidersDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut aot_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);
    let mut demo = weak.upgrade().ok_or("missing mesh collider demo")?;

    for (mode, steps) in [(1, 20), (2, 20), (0, 100)] {
        demo.invoke_launch(mode);
        interpreted.invoke("launch", &[Value::Int(mode)])?;
        assert!(
            demo.get_terrain_body() > 0 && demo.get_panels_body() > 0 && demo.get_projectile() > 0
        );
        if mode == 0 {
            demo.set_running(true);
            interpreted.set("running", Value::Bool(true))?;
        }
        for _ in 0..steps {
            demo.invoke_advance();
            interpreted.invoke("advance", &[])?;
            if mode == 0 && !demo.get_running() {
                break;
            }
        }
        let position = demo.get_position();
        match mode {
            1 => assert!(position.z > 1.0, "open gap: {position:?}"),
            2 => assert!(position.z < -0.12, "mirrored glTF panel: {position:?}"),
            _ => {
                assert!(!demo.get_running());
                assert_eq!(interpreted.get("running")?, Value::Bool(false));
                let surface = demo.get_surface();
                assert!(surface.valid);
                let clearance = position.y - surface.position.y;
                assert!(
                    (clearance - 0.25).abs() < 0.06,
                    "terrain clearance: {clearance}"
                );
            }
        }
        assert_eq!(interpreted.get("position")?, Value::Point3D(position));
        assert_eq!(
            interpreted.get("angles")?,
            Value::Vector3D(demo.get_angles())
        );
        assert_eq!(
            aot_view.render(&mut aot_gpu)?,
            interpreter_view.render(&mut interpreter_gpu)?
        );
    }
    Ok(())
}

#[test]
fn compound_bodies_preserve_the_gate_opening_and_trigger_identity() -> TestResult {
    use compound_bodies_ui::CompoundBodiesDemo;

    const NAME: &str = "examples/puzzle3d/compound_bodies.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("CompoundBodiesDemo")?;
    let demo = CompoundBodiesDemo::new();
    let weak = demo.as_weak();
    let mut aot_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut aot_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);
    let mut demo = weak.upgrade().ok_or("missing compound body demo")?;
    let mut frame = || -> TestResult<Vec<u8>> {
        let png = aot_view.render(&mut aot_gpu)?.to_vec();
        assert_eq!(
            png.as_slice(),
            interpreter_view.render(&mut interpreter_gpu)?
        );
        Ok(png)
    };
    let check_lane = |demo: &mut CompoundBodiesDemo, open| -> TestResult {
        demo.invoke_launch(open);
        interpreted.invoke("launch", &[Value::Bool(open)])?;
        assert!(demo.get_gate() != 0 && demo.get_sensor() != 0 && demo.get_ball() != 0);
        for _ in 0..43 {
            demo.invoke_advance();
            interpreted.invoke("advance", &[])?;
        }
        let position = demo.get_ball_position();
        if open {
            assert!(position.x > 2.4, "{position:?}");
            assert_eq!(demo.get_entries(), 1);
            assert_eq!(demo.get_exits(), 1);
        } else {
            assert!(position.x < -0.36, "{position:?}");
            assert_eq!(demo.get_entries(), 0);
            assert_eq!(demo.get_exits(), 0);
        }
        assert_eq!(interpreted.get("ball-position")?, Value::Point3D(position));
        assert_eq!(interpreted.get("entries")?, Value::Int(demo.get_entries()));
        assert_eq!(interpreted.get("exits")?, Value::Int(demo.get_exits()));
        Ok(())
    };
    check_lane(&mut demo, true)?;
    let through = frame()?;
    check_lane(&mut demo, false)?;
    let blocked = frame()?;
    assert_ne!(through, blocked);
    demo.invoke_set_gate_raised(true);
    interpreted.invoke("set-gate-raised", &[Value::Bool(true)])?;
    demo.invoke_advance();
    interpreted.invoke("advance", &[])?;
    let gate = demo.get_gate_position();
    assert!((gate.y - 1.1).abs() < 0.001);
    assert_eq!(interpreted.get("gate-position")?, Value::Point3D(gate));
    assert_ne!(blocked, frame()?);
    Ok(())
}

#[test]
fn joint_mechanisms_match_in_aot_and_interpreter() -> TestResult {
    use physics_joints_ui::PhysicsJointsDemo;

    const NAME: &str = "examples/puzzle3d/physics_joints.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted = program.create("PhysicsJointsDemo")?;
    let demo = PhysicsJointsDemo::new();
    let weak = demo.as_weak();
    let mut native_view = PreviewScene::new(Box::new(demo));
    let mut interpreter_view = PreviewScene::new(Box::new(interpreted.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut native_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);
    let mut demo = weak.upgrade().ok_or("missing joint mechanisms demo")?;
    let mut snapshot = || -> TestResult<Vec<u8>> {
        let image = native_view.render(&mut native_gpu)?.to_vec();
        assert_eq!(
            image.as_slice(),
            interpreter_view.render(&mut interpreter_gpu)?
        );
        Ok(image)
    };
    let advance = |demo: &mut PhysicsJointsDemo, steps| -> TestResult {
        for _ in 0..steps {
            demo.invoke_advance();
            interpreted.invoke("advance", &[])?;
        }
        Ok(())
    };

    demo.invoke_reset();
    interpreted.invoke("reset", &[])?;
    for joint in [
        demo.get_fixed_joint(),
        demo.get_hinge(),
        demo.get_rail(),
        demo.get_ball_joint(),
        demo.get_tether(),
    ] {
        assert!(joint > 0);
    }
    let initial_image = snapshot()?;
    let knob = demo.get_knob_position();
    let swing = demo.get_swing_position();
    demo.invoke_nudge();
    interpreted.invoke("nudge", &[])?;
    advance(&mut demo, 36)?;
    assert!(demo.get_swing_position().x > swing.x + 0.3);
    assert!((demo.get_tether_span() - 1.35).abs() < 0.025);
    assert_eq!(
        interpreted.get("swing-position")?,
        Value::Point3D(demo.get_swing_position())
    );
    assert_eq!(
        interpreted.get("tether-span")?,
        Value::Float(demo.get_tether_span())
    );
    advance(&mut demo, 110)?;
    assert!(
        (demo.get_door_angle() - 70.0).abs() < 1.5,
        "{}",
        demo.get_door_angle()
    );
    assert!((demo.get_slider_travel() - 1.2).abs() < 0.025);
    assert!(demo.get_knob_position().z < knob.z - 0.8);
    assert_eq!(
        interpreted.get("knob-position")?,
        Value::Point3D(demo.get_knob_position())
    );
    assert_ne!(initial_image, snapshot()?);

    demo.invoke_tighten();
    interpreted.invoke("tighten", &[])?;
    advance(&mut demo, 90)?;
    assert!((demo.get_door_angle() - 25.0).abs() < 1.5);
    assert!((demo.get_slider_travel() - 0.5).abs() < 0.025);
    demo.invoke_drive(false);
    interpreted.invoke("drive", &[Value::Bool(false)])?;
    advance(&mut demo, 90)?;
    assert!((demo.get_door_angle() + 20.0).abs() < 1.5);
    assert!((demo.get_slider_travel() + 0.5).abs() < 0.025);
    assert_eq!(
        interpreted.get("door-angle")?,
        Value::Float(demo.get_door_angle())
    );
    assert_eq!(
        interpreted.get("slider-travel")?,
        Value::Float(demo.get_slider_travel())
    );
    let _ = snapshot()?;
    let post = demo.get_post();
    let door = demo.get_door();
    let hinge = demo.get_hinge();
    let rail = demo.get_rail();
    let knob_joint = demo.get_fixed_joint();
    let door_pose = demo.get_door_position();
    let slider_pose = demo.get_slider_position();
    let apply = |demo: &mut PhysicsJointsDemo, broken| -> TestResult {
        demo.invoke_apply_rig(true, broken);
        interpreted.invoke("apply-rig", &[Value::Bool(true), Value::Bool(broken)])?;
        assert_eq!(demo.get_accepted(), !broken);
        for (name, handle) in [
            ("post", demo.get_post()),
            ("door", demo.get_door()),
            ("rail", demo.get_rail()),
            ("hinge", demo.get_hinge()),
            ("fixed-joint", demo.get_fixed_joint()),
        ] {
            assert_eq!(interpreted.get(name)?, Value::Int(handle));
        }
        assert_eq!(demo.get_door(), door);
        assert_eq!(demo.get_rail(), rail);
        assert_eq!(demo.get_fixed_joint(), knob_joint);
        assert_eq!(demo.get_door_position(), door_pose);
        assert_eq!(demo.get_slider_position(), slider_pose);
        Ok(())
    };
    apply(&mut demo, true)?;
    assert_eq!(demo.get_post(), post);
    assert_eq!(demo.get_hinge(), hinge);
    assert!(!demo.get_wide_post());
    let rejected = snapshot()?;
    apply(&mut demo, false)?;
    assert_ne!(demo.get_post(), post);
    assert_ne!(demo.get_hinge(), hinge);
    assert!(demo.get_wide_post());
    assert_ne!(snapshot()?, rejected);
    Ok(())
}

#[test]
fn sphere_queries_stop_at_edges_and_match_the_interpreter_on_heightfields() -> TestResult {
    use sphere_cast_ui::SphereCastDemo;

    const NAME: &str = "examples/puzzle3d/sphere_cast.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("SphereCastDemo")?;
    let app = SphereCastDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let render = |aot: &mut PreviewScene,
                  interpreter: &mut PreviewScene,
                  gpu: &mut Renderer,
                  other_gpu: &mut Renderer|
     -> TestResult<Vec<u8>> {
        let png = aot.render(gpu)?.to_vec();
        assert!(png.as_slice() == interpreter.render(other_gpu)?);
        Ok(xrack_assets::decode_png(&png)?.rgba[..604 * 354 * 4].to_vec())
    };
    let mut app = weak.upgrade().ok_or("missing sweep demo")?;
    let check = |generated: &SphereCastDemo| -> TestResult {
        assert_eq!(
            Value::Hit3D(generated.get_contact()),
            dynamic.get("contact")?
        );
        assert_eq!(
            Value::Hit3D(generated.get_point_ray()),
            dynamic.get("point-ray")?
        );
        assert_eq!(
            Value::Hit3D(generated.get_terrain_hit()),
            dynamic.get("terrain-hit")?
        );
        let Value::Hit3DList(rows) = dynamic.get("overlap")? else {
            return Err("missing overlap list".into());
        };
        let aot_rows = generated.get_overlap();
        assert_eq!(aot_rows.len(), rows.len());
        for index in 0..rows.len() {
            assert_eq!(aot_rows.get(index).cloned(), rows.row(index));
        }
        Ok(())
    };

    check(&app)?;
    let edge = app.get_contact();
    assert_eq!(edge.object_id, "wall");
    assert_eq!(edge.row_index, 0);
    assert!(!app.get_point_ray().valid);
    let expected = 2.8 - 0.21_f64.sqrt();
    assert!((edge.distance - expected).abs() < 1e-6, "{edge:?}");
    assert!((edge.position.x + 0.2).abs() < 1e-6);
    assert!((edge.position.z - 0.7).abs() < 1e-6);
    assert!((edge.normal.z - 0.4).abs() < 1e-6);
    let terrain = app.get_terrain_hit();
    assert_eq!(terrain.object_id, "terrain");
    assert_eq!(terrain.row_index, 2);
    assert!(terrain.normal.y > 0.5);
    assert!((4.0 - terrain.distance - terrain.position.y - 0.3 * terrain.normal.y).abs() < 1e-6);
    let rows = app.get_overlap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows.first().ok_or("wall overlap")?.object_id, "wall");
    assert_eq!(rows.get(1).ok_or("gate overlap")?.object_id, "gate");
    let blocked = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    let meshes = gpu.scene_mesh_uploads();

    app.set_radius(0.1);
    dynamic.set("radius", Value::Float(0.1))?;
    check(&app)?;
    assert!(!app.get_contact().valid);
    let passed = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(blocked != passed);
    assert_eq!(meshes, gpu.scene_mesh_uploads());

    app.set_proxy(true);
    dynamic.set("proxy", Value::Bool(true))?;
    check(&app)?;
    let hull = app.get_contact();
    assert_eq!(hull.object_id, "wall");
    assert_eq!(hull.row_index, 0);
    assert!((hull.distance - 2.2).abs() < 1e-6, "{hull:?}");
    assert!(!app.get_point_ray().valid);
    let with_proxy = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(with_proxy != passed);
    assert_eq!(meshes, gpu.scene_mesh_uploads());

    app.invoke_resize_proxy(2.0);
    dynamic.invoke("resize-proxy", &[Value::Float(2.0)])?;
    check(&app)?;
    assert!((app.get_contact().distance - 2.0).abs() < 1e-6);
    let resized = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(with_proxy != resized);
    assert_eq!(meshes, gpu.scene_mesh_uploads());

    app.set_capsule_proxy(true);
    app.set_lane(0.84);
    dynamic.set("capsule-proxy", Value::Bool(true))?;
    dynamic.set("lane", Value::Float(0.84))?;
    check(&app)?;
    let capsule = app.get_contact();
    assert_eq!(capsule.object_id, "wall");
    assert!((capsule.distance - 2.2).abs() < 1e-6, "{capsule:?}");
    assert!(!app.get_point_ray().valid);
    assert!(resized != render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?);
    assert_eq!(meshes, gpu.scene_mesh_uploads());

    app.set_proxy(false);
    dynamic.set("proxy", Value::Bool(false))?;
    app.set_lane(0.2);
    app.set_radius(0.5);
    app.set_blocking(false);
    dynamic.set("lane", Value::Float(0.2))?;
    dynamic.set("radius", Value::Float(0.5))?;
    dynamic.set("blocking", Value::Bool(false))?;
    check(&app)?;
    assert!(!app.get_contact().valid);
    assert_eq!(app.get_point_ray().object_id, "wall");
    assert_eq!(app.get_overlap().len(), 1);

    app.set_mask(2);
    app.set_lane(0.9);
    dynamic.set("mask", Value::Int(2))?;
    dynamic.set("lane", Value::Float(0.9))?;
    check(&app)?;
    assert_eq!(app.get_contact().object_id, "gate");
    assert!((app.get_contact().distance - 3.775).abs() < 1e-6);
    app.set_requested(1.0);
    dynamic.set("requested", Value::Float(1.0))?;
    check(&app)?;
    assert!(!app.get_contact().valid);
    Ok(())
}

#[test]
fn fog_shapes_and_world_height_profiles_share_extinction_in_both_backends() -> TestResult {
    use fog_profiles_ui::FogProfilesDemo;
    use xrack_runtime::{
        Color,
        scene3d::{Point3D, linear_color},
    };

    const NAME: &str = "examples/puzzle3d/fog_profiles.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("FogProfilesDemo")?;
    let app = FogProfilesDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    #[derive(Clone, Copy)]
    struct Case {
        profiles: bool,
        shape: &'static str,
        angle: f64,
        pitch: f64,
        height: f64,
        height_end: f64,
        flipped: bool,
        overlap: bool,
        reversed: bool,
        lit: bool,
    }
    let plain = Case {
        profiles: false,
        shape: "box",
        angle: 0.0,
        pitch: 0.0,
        height: 0.35,
        height_end: -0.35,
        flipped: false,
        overlap: false,
        reversed: false,
        lit: false,
    };
    let mut render = |case: Case| -> TestResult<(xrack_assets::Raster, u64)> {
        let mut app = weak.upgrade().ok_or("missing fog profile scene")?;
        app.set_profiles(case.profiles);
        app.set_shape(case.shape.into());
        app.set_angle(case.angle);
        app.set_camera_pitch(case.pitch);
        app.set_height_start(case.height);
        app.set_height_end(case.height_end);
        app.set_flipped(case.flipped);
        app.set_overlap(case.overlap);
        app.set_reversed(case.reversed);
        app.set_lit(case.lit);
        for (name, value) in [
            ("profiles", Value::Bool(case.profiles)),
            ("shape", Value::String(case.shape.into())),
            ("angle", Value::Float(case.angle)),
            ("camera-pitch", Value::Float(case.pitch)),
            ("height-start", Value::Float(case.height)),
            ("height-end", Value::Float(case.height_end)),
            ("flipped", Value::Bool(case.flipped)),
            ("overlap", Value::Bool(case.overlap)),
            ("reversed", Value::Bool(case.reversed)),
            ("lit", Value::Bool(case.lit)),
        ] {
            dynamic.set(name, value)?;
        }
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == interpreter.render(&mut other_gpu)?);
        Ok((
            xrack_assets::decode_png(&png)?,
            gpu.scene_draw_stats().viewport_cache_hits,
        ))
    };
    let pixel = |raster: &xrack_assets::Raster, [x, y]: [usize; 2]| -> [u8; 4] {
        let offset = (y * raster.size[0] as usize + x) * 4;
        std::array::from_fn(|axis| raster.rgba[offset + axis])
    };
    let camera = SceneCamera::new(0.0, 0.0, 10.0, [600.0, 356.0]).ok_or("camera")?;
    let sample = |raster: &xrack_assets::Raster, x, y| {
        let at = camera.project(Point3D::new(x, y, 0.0)).point;
        pixel(raster, [at.x.0 as usize, at.y.0 as usize])
    };
    let (box_upright, _) = render(plain)?;
    let background = pixel(&box_upright, [0, 0]);
    let (box_tilted, _) = render(Case {
        angle: 45.0,
        ..plain
    })?;
    assert_eq!(sample(&box_upright, 1.85, 1.6), background);
    assert_ne!(sample(&box_tilted, 1.85, 1.6), background);
    assert_eq!(sample(&box_tilted, 1.85, -1.6), background);
    let (sphere_upright, _) = render(Case {
        shape: "sphere",
        ..plain
    })?;
    let (sphere_tilted, _) = render(Case {
        angle: 45.0,
        shape: "sphere",
        ..plain
    })?;
    assert_eq!(sample(&sphere_upright, 1.85, 1.6), background);
    assert_ne!(sample(&sphere_tilted, 1.85, 1.6), background);

    let height = Case {
        profiles: true,
        ..plain
    };
    let (graded, _) = render(height)?;
    let (inverted, _) = render(Case {
        flipped: true,
        ..height
    })?;
    assert_eq!(sample(&graded, 0.0, 0.85), background);
    assert_eq!(sample(&inverted, 0.0, -0.85), background);
    assert_eq!(
        sample(&graded, 0.0, -0.85),
        sample(&box_upright, 0.0, -0.85)
    );
    assert_eq!(
        sample(&inverted, 0.0, 0.85),
        sample(&box_upright, 0.0, 0.85)
    );
    let (equal_planes, _) = render(Case {
        height_end: height.height,
        ..height
    })?;
    assert!(equal_planes.rgba[..600 * 356 * 4] == box_upright.rgba[..600 * 356 * 4]);

    // Through the center ray at 25 degrees, the box is 2/cos(25) units deep.
    // The symmetric profile crosses both ramp planes, with average density 0.35.
    let (looking_down, _) = render(Case {
        pitch: 25.0,
        ..height
    })?;
    let actual = pixel(&looking_down, [300, 178]);
    let transmission = (-0.7_f32 / 25.0_f32.to_radians().cos()).exp();
    let rgb = linear_color(Color(0x203346ff));
    let tint = linear_color(Color(0x71cee4ff));
    for axis in 0..3 {
        let channel = tint[axis] + (rgb[axis] - tint[axis]) * transmission;
        let expected = (1.055 * channel.powf(1.0 / 2.4) - 0.055) * 255.0;
        assert!(
            (f32::from(actual[axis]) - expected).abs() <= 2.0,
            "{actual:?}"
        );
    }

    let mixed = Case {
        pitch: 25.0,
        angle: -22.0,
        overlap: true,
        ..height
    };
    let (combined, _) = render(mixed)?;
    let (reversed, _) = render(Case {
        reversed: true,
        ..mixed
    })?;
    let viewport = &combined.rgba[..600 * 356 * 4];
    assert!(
        viewport
            .iter()
            .zip(&reversed.rgba)
            .all(|(&a, &b)| a.abs_diff(b) <= 1)
    );
    let lit = Case { lit: true, ..mixed };
    let (scatter, cold) = render(lit)?;
    assert!(scatter.rgba[..viewport.len()] != *viewport);
    let (warm, cache) = render(lit)?;
    assert!(warm.rgba == scatter.rgba);
    assert_eq!(cache, cold + 1);
    Ok(())
}

#[test]
fn local_fog_integrates_space_up_to_the_visible_surface_in_both_backends() -> TestResult {
    use volumetric_fog_ui::VolumetricFogDemo;
    use xrack_runtime::{
        Color,
        scene3d::{Point3D, linear_color},
    };

    const NAME: &str = "examples/puzzle3d/volumetric_fog.xr";
    const VIEW_BYTES: usize = 600 * 354 * 4;
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("VolumetricFogDemo")?;
    let app = VolumetricFogDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let camera = SceneCamera::new(0.0, 0.0, 10.0, [600.0, 354.0]).ok_or("camera")?;
    let sample = |pixels: &[u8], position| {
        let point = camera.project(position).point;
        let offset = (point.y.0 as usize * 600 + point.x.0 as usize) * 4;
        <[u8; 4]>::try_from(&pixels[offset..offset + 4])
    };
    let front = Point3D::new(-0.84, 0.16, 3.5);
    let inside = Point3D::new(0.3, -0.12, 1.1);
    let back = Point3D::new(1.25, -0.2, -2.7);
    let render = |aot: &mut PreviewScene,
                  interpreter: &mut PreviewScene,
                  gpu: &mut Renderer,
                  other_gpu: &mut Renderer|
     -> TestResult<Vec<u8>> {
        let png = aot.render(gpu)?.to_vec();
        assert!(png.as_slice() == interpreter.render(other_gpu)?);
        Ok(xrack_assets::decode_png(&png)?.rgba[..VIEW_BYTES].to_vec())
    };
    let mut app = weak.upgrade().ok_or("missing volume fog example")?;
    app.set_fog_enabled(false);
    dynamic.set("fog-enabled", Value::Bool(false))?;
    let plain = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    let uploads = [gpu.scene_mesh_uploads(), gpu.scene_instance_uploads()];
    let draws = gpu.scene_draw_stats().color_draws;

    app.set_fog_enabled(true);
    dynamic.set("fog-enabled", Value::Bool(true))?;
    let fog = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert_eq!(sample(&fog, front)?, sample(&plain, front)?);
    assert_ne!(sample(&fog, inside)?, sample(&plain, inside)?);
    assert_ne!(sample(&fog, back)?, sample(&plain, back)?);
    assert_eq!(fog[..4], plain[..4]);
    assert_eq!(app.get_picked(), "front");
    assert_eq!(dynamic.get("picked")?, Value::String("front".into()));
    assert_eq!(
        [gpu.scene_mesh_uploads(), gpu.scene_instance_uploads()],
        uploads
    );
    assert_eq!(gpu.scene_draw_stats().color_draws - draws, draws);

    let before = gpu.scene_draw_stats();
    assert_eq!(
        fog,
        render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?
    );
    assert_eq!(
        gpu.scene_draw_stats().viewport_cache_hits,
        before.viewport_cache_hits + 1
    );

    app.set_reverse_list(true);
    dynamic.set("reverse-list", Value::Bool(true))?;
    let reversed = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(reversed.iter().zip(&fog).all(|(&a, &b)| a.abs_diff(b) <= 1));

    app.set_view_mask(1);
    dynamic.set("view-mask", Value::Int(1))?;
    let first_layer = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert_ne!(sample(&first_layer, back)?, sample(&fog, back)?);
    app.set_overlap(false);
    app.set_view_mask(3);
    dynamic.set("overlap", Value::Bool(false))?;
    dynamic.set("view-mask", Value::Int(3))?;
    let one_volume = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert!(one_volume == first_layer);

    app.set_geometry(false);
    dynamic.set("geometry", Value::Bool(false))?;
    let empty = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert_ne!(sample(&empty, front)?, sample(&plain, front)?);
    assert_eq!(empty[..4], plain[..4]);
    let center = sample(&empty, Point3D::new(0.0, 0.0, 0.0))?;
    let transmission = (-2.0_f32 * 0.65).exp();
    let background = linear_color(Color(0x203346ff));
    let fog = linear_color(Color(0x65c7e0ff));
    for axis in 0..3 {
        let expected = fog[axis] + (background[axis] - fog[axis]) * transmission;
        let srgb = (1.055 * expected.powf(1.0 / 2.4) - 0.055) * 255.0;
        assert!((f32::from(center[axis]) - srgb).abs() <= 2.0, "{center:?}");
    }
    app.set_hdr_sky(true);
    dynamic.set("hdr-sky", Value::Bool(true))?;
    let hdr_with_fog = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    app.set_fog_enabled(false);
    dynamic.set("fog-enabled", Value::Bool(false))?;
    let hdr_sky = render(&mut aot, &mut interpreter, &mut gpu, &mut other_gpu)?;
    assert_eq!(hdr_with_fog[..4], hdr_sky[..4]);
    assert_ne!(
        sample(&hdr_with_fog, Point3D::new(0.0, 0.0, 0.0))?,
        sample(&hdr_sky, Point3D::new(0.0, 0.0, 0.0))?
    );
    Ok(())
}

#[test]
fn fog_profiles_update_material_color_and_reuse_geometry_in_both_backends() -> TestResult {
    use fog_ui::SceneFogDemo;
    use xrack_runtime::scene3d::Point3D;

    const NAME: &str = "examples/puzzle3d/fog.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("SceneFogDemo")?;
    let app = SceneFogDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut aot_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);
    let camera = SceneCamera::new(17.0, 11.0, 10.0, [600.0, 354.0]).ok_or("missing camera")?;
    let sample_points = [
        Point3D::new(-1.65, -0.47, 1.35),
        Point3D::new(-1.65, 1.52, 1.35),
        Point3D::new(1.55, -0.47, -3.15),
    ]
    .map(|point| camera.project(point).point);
    #[derive(Clone, Copy)]
    struct Profile {
        height: bool,
        depth: bool,
        transmit: bool,
        reversed: bool,
        height_curve: f64,
        depth_curve: f64,
        height_end: f64,
    }
    let clear = Profile {
        height: false,
        depth: false,
        transmit: false,
        reversed: false,
        height_curve: 1.0,
        depth_curve: 1.0,
        height_end: -0.9,
    };
    let mut render = |profile: Profile| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing fog demo")?;
        app.set_height_fog(profile.height);
        app.set_depth_fog(profile.depth);
        app.set_light_transmission(profile.transmit);
        app.set_reversed(profile.reversed);
        app.set_height_curve(profile.height_curve);
        app.set_depth_curve(profile.depth_curve);
        app.set_height_end(profile.height_end);
        for (name, value) in [
            ("height-fog", Value::Bool(profile.height)),
            ("depth-fog", Value::Bool(profile.depth)),
            ("light-transmission", Value::Bool(profile.transmit)),
            ("reversed", Value::Bool(profile.reversed)),
            ("height-curve", Value::Float(profile.height_curve)),
            ("depth-curve", Value::Float(profile.depth_curve)),
            ("height-end", Value::Float(profile.height_end)),
        ] {
            dynamic.set(name, value)?;
        }
        let image = generated.render(&mut aot_gpu)?.to_vec();
        assert!(image.as_slice() == interpreted.render(&mut interpreter_gpu)?);
        assert_eq!(app.get_picked(), "near-high");
        assert_eq!(dynamic.get("picked")?, Value::String("near-high".into()));
        let rgba = xrack_assets::decode_png(&image)?.rgba;
        let sample = |x, y| {
            let i = 4 * (600 * y + x);
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        let pixels = sample_points.map(|point| sample(point.x.0 as usize, point.y.0 as usize));
        let stats = aot_gpu.scene_draw_stats();
        Ok((
            pixels,
            sample(8, 8),
            [
                aot_gpu.scene_mesh_uploads(),
                aot_gpu.scene_instance_uploads(),
                stats.viewport_renders,
                stats.viewport_cache_hits,
                stats.color_draws,
            ],
        ))
    };

    let (plain, sky, cold) = render(clear)?;
    let (_, _, warm) = render(clear)?;
    assert_eq!(warm[2], cold[2]);
    assert_eq!(warm[3], cold[3] + 1);
    let check = |after: [u64; 5]| {
        assert_eq!(after[..2], cold[..2], "fog only updates viewport shading");
    };
    let height = Profile {
        height: true,
        ..clear
    };
    let (low_fog, background, updated) = render(height)?;
    assert_eq!(background, sky);
    assert_eq!(
        updated[4] - warm[4],
        cold[4],
        "fog uses the regular color draws"
    );
    check(updated);
    assert!(low_fog[0][0] < plain[0][0]);
    assert_eq!(low_fog[1], plain[1]);
    let (curved, _, updated) = render(Profile {
        height_curve: 3.0,
        ..height
    })?;
    check(updated);
    assert!(curved[0][0] > low_fog[0][0]);

    let (inverted, _, _) = render(Profile {
        reversed: true,
        height_end: -0.15,
        ..height
    })?;
    assert_eq!(inverted[0], plain[0]);
    assert!(inverted[1][0] < plain[1][0]);
    let (degenerate, _, _) = render(Profile {
        height_end: 0.95,
        ..height
    })?;
    assert_eq!(degenerate, plain);

    let depth = Profile {
        depth: true,
        ..clear
    };
    let (distance, _, _) = render(depth)?;
    assert!(distance[2][0] < plain[2][0]);
    let (curved_depth, _, _) = render(Profile {
        depth_curve: 3.0,
        ..depth
    })?;
    assert!(curved_depth[2][0] > distance[2][0]);
    let (luminous, _, _) = render(Profile {
        transmit: true,
        ..depth
    })?;
    assert!(luminous[2][0] > distance[2][0]);
    let (together, _, final_stats) = render(Profile {
        height: true,
        ..depth
    })?;
    assert!(together[0][0] <= distance[0][0].min(low_fog[0][0]));
    check(final_stats);
    Ok(())
}

#[test]
fn viewport_quality_resolves_coverage_and_fogs_by_depth_in_both_backends() -> TestResult {
    const NAME: &str = "examples/puzzle3d/quality.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("QualityDemo")?;
    let app = quality_ui::QualityDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    let mut render = |smooth, fog, hdr, show_glass, far| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing viewport quality demo")?;
        app.set_smooth(smooth);
        app.set_fog(fog);
        app.set_hdr(hdr);
        app.set_show_glass(show_glass);
        app.set_fog_far(far);
        for (name, value) in [
            ("smooth", Value::Bool(smooth)),
            ("fog", Value::Bool(fog)),
            ("hdr", Value::Bool(hdr)),
            ("show-glass", Value::Bool(show_glass)),
            ("fog-far", Value::Float(far)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = renderer.scene_draw_stats();
        let image = generated.render(&mut renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert!(
            image.as_slice() == interpreted.render(&mut renderer)?,
            "viewport quality AOT and interpreter pixels differ"
        );
        assert_eq!(
            after.multisample_resolves - before.multisample_resolves,
            u64::from(smooth)
        );
        assert_eq!(app.get_picked(), "near");
        assert_eq!(dynamic.get("picked")?, Value::String("near".into()));
        Ok((
            xrack_assets::decode_png(&image)?,
            after.color_draws - before.color_draws,
            renderer.scene_instance_uploads(),
        ))
    };

    let [width, height] = [480, 248];
    let (baseline, draws, uploads) = render(false, false, false, false, 10.0)?;
    let (msaa, msaa_draws, after_msaa) = render(true, false, false, false, 10.0)?;
    assert_eq!(baseline.size, [480, 300]);
    assert_eq!(
        draws, msaa_draws,
        "MSAA resolves in the existing color pass"
    );
    assert_eq!(
        uploads, after_msaa,
        "MSAA reuses populated instance buffers"
    );
    let background = [0x17, 0x23, 0x33, 0xff];
    let viewport_bytes = width * height * 4;
    let (single_pixels, _) = baseline.rgba[..viewport_bytes].as_chunks::<4>();
    let (multi_pixels, _) = msaa.rgba[..viewport_bytes].as_chunks::<4>();
    let new_coverage = single_pixels
        .iter()
        .zip(multi_pixels)
        .filter(|(single, multi)| **single == background && **multi != background)
        .count();
    assert!(
        new_coverage > 20,
        "subpixel silhouette coverage: {new_coverage}"
    );

    let sample = |image: &xrack_assets::Raster, x, y| {
        let offset = 4 * (width * y + x);
        let color: [u8; 4] = std::array::from_fn(|index| image.rgba[offset + index]);
        color
    };
    let (fog, fog_draws, after_fog) = render(false, true, false, false, 10.0)?;
    assert_eq!(sample(&fog, 0, 0), background);
    assert_eq!(sample(&fog, 160, 124), sample(&baseline, 160, 124));
    assert_eq!(sample(&fog, 305, 124), [0x4e, 0x80, 0xb3, 0xff]);
    assert_ne!(sample(&baseline, 305, 124), sample(&fog, 305, 124));
    assert_eq!(draws, fog_draws);
    assert_eq!(uploads, after_fog);
    let (empty_range, _, _) = render(false, true, false, false, 8.0)?;
    assert!(baseline.rgba[..viewport_bytes] == empty_range.rgba[..viewport_bytes]);

    // An HDR sky and a blended glTF material must share the same resolve.
    let (hdr, hdr_draws, hdr_uploads) = render(false, true, true, true, 10.0)?;
    let (hdr_msaa, hdr_msaa_draws, hdr_msaa_uploads) = render(true, true, true, true, 10.0)?;
    assert!(hdr_draws > draws, "the blended material is drawn");
    assert_eq!(hdr_draws, hdr_msaa_draws);
    assert_eq!(hdr_uploads, hdr_msaa_uploads);
    assert_eq!(sample(&hdr, 240, 124), sample(&hdr_msaa, 240, 124));
    assert!(hdr.rgba[..viewport_bytes] != hdr_msaa.rgba[..viewport_bytes]);
    eprintln!("4× MSAA added subpixel coverage to {new_coverage} silhouette pixels");
    Ok(())
}

#[test]
fn stationary_viewport_accumulates_linear_color_and_reuses_it_in_both_backends() -> TestResult {
    const NAME: &str = "examples/puzzle3d/quality.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("QualityDemo")?;
    let generated = quality_ui::QualityDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;

    struct Frame {
        viewport: Vec<u8>,
        work: [u64; 2],
        pending: bool,
    }
    let viewport_bytes = 480 * 248 * 4;
    let mut draw = |samples, yaw, hdr, supersampling| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing viewport quality demo")?;
        app.set_aa_frames(samples);
        app.set_yaw(yaw);
        app.set_hdr(hdr);
        app.set_aa_scale(supersampling);
        for (name, value) in [
            ("aa-frames", Value::Int(samples)),
            ("yaw", Value::Float(yaw)),
            ("hdr", Value::Bool(hdr)),
            ("aa-scale", Value::Float(supersampling)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render_animated(&mut gpu)?.to_vec();
        assert!(png.as_slice() == interpreted.render_animated(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        let pending = aot.next_frame_delay().is_some();
        assert_eq!(pending, interpreted.next_frame_delay().is_some());
        assert_eq!(pending, gpu.needs_redraw());
        assert_eq!(app.get_picked(), "near");
        assert_eq!(dynamic.get("picked")?, Value::String("near".into()));
        Ok(Frame {
            viewport: xrack_assets::decode_png(&png)?.rgba[..viewport_bytes].to_vec(),
            work: [
                after.viewport_renders - before.viewport_renders,
                after.progressive_aa_samples - before.progressive_aa_samples,
            ],
            pending,
        })
    };

    let baseline = draw(0, 0.0, false, 1.0)?;
    let unsupported = draw(3, 0.0, false, 1.0)?;
    assert!(!baseline.pending && !unsupported.pending);
    assert_eq!(baseline.viewport, unsupported.viewport);
    for samples in [2, 4, 8] {
        let centered = draw(samples, 0.0, false, 1.0)?;
        assert_eq!(centered.work, [1, 0]);
        assert!(centered.pending);
        assert_eq!(centered.viewport, baseline.viewport);
        let mut image = centered.viewport;
        for index in 0..samples {
            let accumulated = draw(samples, 0.0, false, 1.0)?;
            assert_eq!(accumulated.work, [1, 1]);
            assert_eq!(accumulated.pending, index + 1 < samples);
            image = accumulated.viewport;
        }
        let background = [0x17, 0x23, 0x33, 0xff];
        let covered = baseline
            .viewport
            .chunks_exact(4)
            .zip(image.chunks_exact(4))
            .filter(|(before, after)| *before == background && *after != background)
            .count();
        assert!(
            covered > 20,
            "{samples} samples added coverage to {covered} pixels"
        );
        let reused = draw(samples, 0.0, false, 1.0)?;
        assert_eq!(reused.work, [0, 0]);
        assert!(!reused.pending);
        assert!(reused.viewport == image);
        eprintln!("{samples} accumulated samples add coverage to {covered} pixels");
    }

    // Moving mid-sequence displays the new centered HDR image without ghosting.
    let reference = draw(0, 16.0, true, 1.5)?;
    assert_eq!(draw(4, 0.0, true, 1.5)?.work, [1, 0]);
    assert_eq!(draw(4, 0.0, true, 1.5)?.work, [1, 1]);
    let moved = draw(4, 16.0, true, 1.5)?;
    assert_eq!(moved.work, [1, 0]);
    assert!(moved.viewport == reference.viewport);
    for index in 0..4 {
        let frame = draw(4, 16.0, true, 1.5)?;
        assert_eq!(frame.work, [1, 1]);
        assert_eq!(frame.pending, index < 3);
    }
    assert_eq!(draw(4, 16.0, true, 1.5)?.work, [0, 0]);
    Ok(())
}

#[test]
fn failed_viewport_sample_restarts_without_partial_history() -> TestResult {
    use xrack_layout::Interaction;
    use xrack_runtime::Length;

    let mut app = quality_ui::QualityDemo::new();
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    app.set_aa_frames(2);
    let scene = app.render();
    let centered = gpu.snapshot(&scene, 480, 300, 1.0, &input)?;
    assert!(gpu.needs_redraw());

    let registration =
        xrack_renderer_femtovg::register_gpu("abort-progressive-aa", |_| Err("bad sample".into()))?;
    let mut broken = Node::new("CustomRender", Some("abort"));
    for (name, value) in [
        ("renderer", Value::String("abort-progressive-aa".into())),
        ("width", Value::Length(Length(24.0))),
        ("height", Value::Length(Length(24.0))),
        ("z", Value::Float(200.0)),
    ] {
        broken.properties.insert(name, value);
    }
    let mut failing = scene.clone();
    failing.children.push(broken);
    let failed = gpu
        .snapshot(&failing, 480, 300, 1.0, &input)
        .err()
        .ok_or("expected failed frame")?;
    assert!(failed.contains("bad sample"));
    assert_eq!(gpu.scene_draw_stats().progressive_aa_samples, 1);
    drop(registration);

    let restarted = gpu.snapshot(&scene, 480, 300, 1.0, &input)?;
    assert!(restarted.rgba == centered.rgba);
    assert!(gpu.needs_redraw());
    assert_eq!(gpu.scene_draw_stats().progressive_aa_samples, 1);
    let _ = gpu.snapshot(&scene, 480, 300, 1.0, &input)?;
    assert!(gpu.needs_redraw());
    let _ = gpu.snapshot(&scene, 480, 300, 1.0, &input)?;
    assert!(!gpu.needs_redraw());
    assert_eq!(gpu.scene_draw_stats().progressive_aa_samples, 3);
    Ok(())
}

#[test]
fn interleaved_preview_scopes_converge_independently() -> TestResult {
    use xrack_layout::Interaction;

    let mut app = quality_ui::QualityDemo::new();
    app.set_aa_frames(2);
    let first = app.render();
    app.set_yaw(16.0);
    let second = app.render();
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    let mut render = |scope, scene: &Node, expected: [u64; 2], pending| -> TestResult {
        gpu.set_scene_observation(scope, false);
        let before = gpu.scene_draw_stats();
        let _ = gpu.snapshot(scene, 480, 300, 1.0, &input)?;
        let after = gpu.scene_draw_stats();
        assert_eq!(
            [
                after.viewport_renders - before.viewport_renders,
                after.progressive_aa_samples - before.progressive_aa_samples,
            ],
            expected
        );
        assert_eq!(gpu.needs_redraw(), pending);
        Ok(())
    };
    for (work, pending) in [
        ([1, 0], true),
        ([1, 1], true),
        ([1, 1], false),
        ([0, 0], false),
    ] {
        render(1, &first, work, pending)?;
        render(2, &second, work, pending)?;
    }
    Ok(())
}

#[test]
fn temporal_aa_follows_motion_then_settles_and_matches_both_backends() -> TestResult {
    use temporal_aa_ui::TemporalAaDemo;

    const NAME: &str = "examples/puzzle3d/temporal_aa.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("TemporalAaDemo")?;
    let app = TemporalAaDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;

    #[derive(Clone, Copy, Debug)]
    struct State {
        angle: f64,
        temporal: bool,
        progressive: bool,
        warm: bool,
        texture_size: i64,
        msaa: bool,
    }
    let initial = State {
        angle: 0.0,
        temporal: false,
        progressive: false,
        warm: true,
        texture_size: 0,
        msaa: false,
    };
    let mut draw = |state: State, work: [u64; 4], pending| -> TestResult<Vec<u8>> {
        let mut app = weak.upgrade().ok_or("missing temporal demo")?;
        app.set_angle(state.angle);
        app.set_temporal(state.temporal);
        app.set_progressive(state.progressive);
        app.set_warm(state.warm);
        app.set_texture_size(state.texture_size);
        app.set_msaa(state.msaa);
        for (name, value) in [
            ("angle", Value::Float(state.angle)),
            ("temporal", Value::Bool(state.temporal)),
            ("progressive", Value::Bool(state.progressive)),
            ("warm", Value::Bool(state.warm)),
            ("texture-size", Value::Int(state.texture_size)),
            ("msaa", Value::Bool(state.msaa)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = generated.render_animated(&mut gpu)?.to_vec();
        assert!(png.as_slice() == interpreted.render_animated(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        assert_eq!(
            [
                after.viewport_renders - before.viewport_renders,
                after.temporal_aa_samples - before.temporal_aa_samples,
                after.temporal_aa_blends - before.temporal_aa_blends,
                after.progressive_aa_samples - before.progressive_aa_samples,
            ],
            work,
            "{state:?}"
        );
        assert_eq!(gpu.needs_redraw(), pending);
        assert_eq!(generated.next_frame_delay().is_some(), pending);
        assert_eq!(interpreted.next_frame_delay().is_some(), pending);
        Ok(png)
    };

    let baseline = draw(initial, [2, 0, 0, 0], false)?;
    let enabled = State {
        temporal: true,
        ..initial
    };
    let _ = draw(enabled, [1, 1, 0, 0], true)?;
    let filtered = draw(enabled, [1, 1, 1, 0], false)?;
    assert!(filtered != baseline);
    assert!(draw(enabled, [0, 0, 0, 0], false)? == filtered);

    // Keep history across both model transforms and a changing orbit camera.
    for angle in [15.0, 42.0, 85.0] {
        let _ = draw(State { angle, ..enabled }, [2, 1, 1, 0], true)?;
    }
    let moved = State {
        angle: 85.0,
        ..enabled
    };
    let settled = draw(moved, [1, 1, 1, 0], false)?;
    let _ = draw(
        State {
            temporal: false,
            ..moved
        },
        [1, 0, 0, 0],
        false,
    )?;
    let _ = draw(moved, [1, 1, 0, 0], true)?;
    let cold_settled = draw(moved, [1, 1, 1, 0], false)?;
    assert!(
        settled == cold_settled,
        "previous motion cannot survive two stable samples"
    );

    let switched = State {
        warm: false,
        ..moved
    };
    let _ = draw(switched, [2, 1, 0, 0], true)?;
    let resized = State {
        texture_size: 192,
        ..switched
    };
    let _ = draw(resized, [1, 1, 0, 0], true)?;
    let _ = draw(resized, [1, 1, 1, 0], false)?;

    let dual = State {
        progressive: true,
        ..resized
    };
    let _ = draw(dual, [1, 1, 0, 0], true)?;
    let dual_moved = State {
        angle: 112.0,
        ..dual
    };
    let _ = draw(dual_moved, [2, 1, 1, 0], true)?;
    for step in 0..4 {
        let _ = draw(dual_moved, [1, 0, 0, 1], step < 3)?;
    }
    let _ = draw(dual_moved, [0, 0, 0, 0], false)?;
    let _ = draw(
        State {
            angle: 125.0,
            ..dual_moved
        },
        [2, 1, 0, 0],
        true,
    )?;
    let _ = draw(
        State {
            msaa: true,
            progressive: false,
            ..dual_moved
        },
        [2, 0, 0, 0],
        false,
    )?;
    Ok(())
}

#[test]
fn heightfield_renders_and_picks_the_same_surface_in_both_backends() -> TestResult {
    use heightfield_ui::HeightfieldDemo;

    const NAME: &str = "examples/puzzle3d/heightfield.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(inputs.iter().any(|path| path.ends_with("models/ridge.png")));
    let dynamic = program.create("HeightfieldDemo")?;
    let app = HeightfieldDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut aot_gpu, mut interpreter_gpu) = (renderer()?, renderer()?);

    let hills = generated.render(&mut aot_gpu)?.to_vec();
    assert_eq!(hills, interpreted.render(&mut interpreter_gpu)?);
    let app = weak.upgrade().ok_or("missing terrain demo")?;
    let initial = app.get_contact();
    assert_eq!(initial.object_id, "ridge");
    assert!(initial.position.y > 1.0 && initial.normal.y > 0.5);
    assert_eq!(dynamic.get("contact")?, Value::Hit3D(initial.clone()));

    let (view, layout) = inspect(&generated)?;
    let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
        return Err("missing terrain scene".into());
    };
    let ground = model
        .iter()
        .find(|item| item.id == "ridge")
        .ok_or("missing terrain")?;
    assert_eq!(ground.mesh.vertices().len(), 64 * 48 * 6);
    assert_eq!(
        ground.mesh.lightmap_uvs().len(),
        ground.mesh.vertices().len()
    );
    let distant =
        SceneCamera::new(30.0, 37.0, 60.0, [64.0, 48.0]).ok_or("missing distant camera")?;
    let reduced = model.geometry(&distant);
    let simplified = reduced
        .iter()
        .find(|item| item.id == "ridge")
        .ok_or("missing simplified terrain")?;
    assert!(simplified.mesh.vertices().len() < ground.mesh.vertices().len());
    assert_eq!(
        simplified.mesh.lightmap_uvs().len(),
        simplified.mesh.vertices().len()
    );

    let bounds = layout.bounds;
    let camera = SceneCamera::new(
        30.0,
        37.0,
        11.0,
        [bounds.width.into(), bounds.height.into()],
    )
    .ok_or("missing orbit camera")?;
    let [x, y, _, w] = camera.clip_position([1.2, 1.3, 0.0]);
    let point = Point {
        x: bounds.x + bounds.width * (1.0 + x / w) * 0.5,
        y: bounds.y + bounds.height * (1.0 - y / w) * 0.5,
    };
    let finger = PointerContact {
        id: 7,
        primary: true,
        event: Default::default(),
    };
    let uploads = aot_gpu.scene_mesh_uploads();
    for scene in [&mut generated, &mut interpreted] {
        assert!(scene.press_contact(finger, point, false));
        assert!(scene.finish_contact(finger, point, true, false));
    }
    let moved = generated.render(&mut aot_gpu)?.to_vec();
    assert_eq!(moved, interpreted.render(&mut interpreter_gpu)?);
    assert_ne!(moved, hills);
    let mut app = weak.upgrade().ok_or("missing terrain demo")?;
    let relocated = app.get_contact();
    assert_eq!(relocated.object_id, "ridge");
    assert!((relocated.position.x - initial.position.x).abs() > 0.5);
    assert_eq!(dynamic.get("contact")?, Value::Hit3D(relocated.clone()));
    assert_eq!(aot_gpu.scene_mesh_uploads(), uploads);

    app.set_flat(true);
    dynamic.set("flat", Value::Bool(true))?;
    let flats = generated.render(&mut aot_gpu)?;
    assert_eq!(flats, interpreted.render(&mut interpreter_gpu)?);
    assert_ne!(flats, moved);
    let flattened = app.get_contact();
    assert_eq!(flattened.object_id, "ridge");
    assert!((flattened.position.y - relocated.position.y / 24.0).abs() < 1e-4);
    assert_eq!(dynamic.get("contact")?, Value::Hit3D(flattened));
    Ok(())
}

#[test]
fn mixed_lightmaps_keep_live_lights_and_unmapped_receivers_in_both_backends() -> TestResult {
    use baked_room_ui::BakedRoomDemo;
    use xrack_runtime::scene3d::{
        BakeRole, LightmapResolution, Ray3D, ViewportEnvironment, raycast,
    };

    const NAME: &str = "examples/puzzle3d/lightmap_baked_view.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("BakedRoomDemo")?;
    let app = BakedRoomDemo::new();
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut other = renderer()?;
    let mut app = weak.upgrade().ok_or("missing baked room")?;
    let mut draw = || -> TestResult<_> {
        let shadow_passes = gpu.scene_shadow_passes();
        let baked_draws = gpu.scene_draw_stats().lightmapped_draws;
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == interpreted.render(&mut other)?);
        let view = xrack_assets::decode_png(&png)?.rgba[..600 * 340 * 4].to_vec();
        assert_eq!(
            gpu.scene_environment_uploads(),
            1,
            "AOT reuses the HDR cubemap"
        );
        assert_eq!(
            other.scene_environment_uploads(),
            1,
            "interpreter reuses the HDR cubemap"
        );
        let stats = gpu.scene_draw_stats();
        assert_eq!(
            stats.lightmap_uploads,
            other.scene_draw_stats().lightmap_uploads
        );
        Ok((
            view,
            gpu.scene_shadow_passes() - shadow_passes,
            stats.lightmapped_draws - baked_draws,
            stats.lightmap_uploads,
        ))
    };
    let (baked, shadow_passes, baked_draws, uploads) = draw()?;
    assert_eq!(shadow_passes, 0);
    assert!(
        baked_draws >= 2,
        "authored and generated charts both receive light"
    );
    assert_eq!(uploads, 1, "mixed resolutions share one GPU atlas");
    let pixel = |frame: &[u8], x: usize, y: usize| -> [u8; 3] {
        let offset = (600 * y + x) * 4;
        std::array::from_fn(|channel| frame[offset + channel])
    };
    let key_intensity = app.get_key_intensity();
    app.set_key_intensity(0.0);
    dynamic.set("key-intensity", Value::Float(0.0))?;
    let (no_key, shadow_passes, _, unchanged) = draw()?;
    assert_eq!(shadow_passes, 0);
    assert_eq!(unchanged, uploads);
    assert_eq!(
        pixel(&baked, 325, 170),
        pixel(&no_key, 325, 170),
        "baked plate"
    );
    assert!(
        pixel(&baked, 265, 160)[0] > pixel(&no_key, 265, 160)[0].saturating_add(60),
        "the unbaked sphere still receives the key light"
    );

    app.set_key_intensity(key_intensity);
    dynamic.set("key-intensity", Value::Float(key_intensity))?;
    app.set_fill_on(false);
    dynamic.set("fill-on", Value::Bool(false))?;
    let (no_fill, shadow_passes, _, unchanged) = draw()?;
    assert_eq!(shadow_passes, 0);
    assert_eq!(unchanged, uploads);
    assert!(
        pixel(&baked, 325, 170)[2] > pixel(&no_fill, 325, 170)[2].saturating_add(35),
        "the baked plate still receives the real-time fill"
    );
    app.set_fill_on(true);
    dynamic.set("fill-on", Value::Bool(true))?;
    app.set_live_shadows(true);
    dynamic.set("live-shadows", Value::Bool(true))?;
    let (with_shadows, shadow_passes, _, unchanged) = draw()?;
    assert!(shadow_passes > 0);
    assert_eq!(unchanged, uploads);
    assert!(
        baked != with_shadows,
        "runtime shadows coexist with the atlas"
    );

    app.set_sphere_shift(1.1);
    dynamic.set("sphere-shift", Value::Float(1.1))?;
    let (moved_shadow, shadow_passes, _, unchanged) = draw()?;
    assert!(
        shadow_passes > 0,
        "the moving sphere updates real-time shadow maps"
    );
    assert_eq!(unchanged, uploads);
    let receiver = pixel(&with_shadows, 348, 128);
    assert!(receiver[2] > pixel(&moved_shadow, 348, 128)[2].saturating_add(25));
    app.set_live_shadows(false);
    dynamic.set("live-shadows", Value::Bool(false))?;
    let (moved_unshadowed, shadow_passes, _, unchanged) = draw()?;
    assert_eq!(shadow_passes, 0);
    assert_eq!(unchanged, uploads);
    assert_eq!(pixel(&baked, 348, 128), pixel(&moved_unshadowed, 348, 128));

    app.set_baked(false);
    dynamic.set("baked", Value::Bool(false))?;
    let (live, _, baked_draws, _) = draw()?;
    assert_eq!(baked_draws, 0);
    assert!(with_shadows != live);
    for preview in [&generated, &interpreted] {
        let (view, _) = inspect(preview)?;
        assert_eq!(view.int("bake-denoise-passes"), Some(2));
        assert_eq!(view.float("bake-indirect-factor"), Some(1.0));
        let environment = ViewportEnvironment::from_node(view).ok_or("missing HDR bake probe")?;
        assert_eq!(environment.intensity, 0.35);
        assert!((environment.yaw.to_degrees() - 25.0).abs() < 0.0001);
        let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
            return Err("missing lightmap role scene model".into());
        };
        for (id, role, resolution) in [
            ("plate", BakeRole::Receiver, LightmapResolution::Size(96)),
            (
                "figure",
                BakeRole::Receiver,
                LightmapResolution::TexelsPerUnit(12.0),
            ),
            ("red", BakeRole::Contributor, LightmapResolution::Inherit),
            ("sphere", BakeRole::Excluded, LightmapResolution::Inherit),
        ] {
            let object = model
                .objects()
                .iter()
                .find(|object| object.id == id)
                .ok_or("missing bake role object")?;
            assert_eq!(object.bake_role, role, "{id}");
            assert_eq!(object.lightmap_resolution, resolution, "{id}");
        }
        let ray = Ray3D::new([0.82, 0.19, 5.0], [0.0, 0.0, -1.0]).ok_or("invalid pick ray")?;
        let hit =
            raycast(model.objects(), ray, 7.0).ok_or("the excluded sphere should be pickable")?;
        assert_eq!(hit.object_id, "sphere");
    }
    Ok(())
}

#[test]
fn lightmaps_keep_the_original_chart_and_batch_distinct_atlas_regions() -> TestResult {
    use lightmaps_ui::LightmapsDemo;

    const NAME: &str = "examples/puzzle3d/lightmaps.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("LightmapsDemo")?;
    let app = LightmapsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;

    #[derive(Clone, Copy, Debug)]
    struct State {
        baked: bool,
        full: bool,
        live: f64,
        gain: f64,
        chart: bool,
        swap: bool,
        msaa: bool,
        ssgi: bool,
    }
    struct Frame {
        view: Vec<u8>,
        draws: u64,
        instances: u64,
        baked: u64,
        uploads: u64,
        compacted: u64,
        ssgi: u64,
    }
    let initial = State {
        baked: true,
        full: true,
        live: 0.0,
        gain: 2.6,
        chart: true,
        swap: false,
        msaa: false,
        ssgi: false,
    };
    let mut draw = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing lightmap demo")?;
        app.set_baked(state.baked);
        app.set_full_diffuse(state.full);
        app.set_live_light(state.live);
        app.set_gain(state.gain);
        app.set_chart(state.chart);
        app.set_swap_tiles(state.swap);
        app.set_msaa(state.msaa);
        app.set_ssgi(state.ssgi);
        for (name, value) in [
            ("baked", Value::Bool(state.baked)),
            ("full-diffuse", Value::Bool(state.full)),
            ("live-light", Value::Float(state.live)),
            ("gain", Value::Float(state.gain)),
            ("chart", Value::Bool(state.chart)),
            ("swap-tiles", Value::Bool(state.swap)),
            ("msaa", Value::Bool(state.msaa)),
            ("ssgi", Value::Bool(state.ssgi)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        assert!(
            image.as_slice() == interpreted.render(&mut preview_gpu)?,
            "{state:?}"
        );
        let after = gpu.scene_draw_stats();
        Ok(Frame {
            view: xrack_assets::decode_png(&image)?.rgba[..600 * 340 * 4].to_vec(),
            draws: after.color_draws - before.color_draws,
            instances: after.color_instances - before.color_instances,
            baked: after.lightmapped_draws - before.lightmapped_draws,
            uploads: after.lightmap_uploads,
            compacted: after.color_compaction_uploads - before.color_compaction_uploads,
            ssgi: after.screen_indirect_resolves - before.screen_indirect_resolves,
        })
    };
    let colors = |image: &Frame, left: bool| {
        let mut warm = 0;
        let mut cool = 0;
        let mut min = u8::MAX;
        let mut max = 0;
        for y in 70..270 {
            let range = if left { 38..282 } else { 318..562 };
            for x in range {
                let offset = 4 * (y * 600 + x);
                let [r, g, b] = [0, 1, 2].map(|channel| image.view[offset + channel]);
                if r > g.saturating_add(20) && r > b.saturating_add(26) {
                    warm += 1;
                    min = min.min(r);
                    max = max.max(r);
                }
                if b > r.saturating_add(40) && b > g.saturating_add(20) {
                    cool += 1;
                }
            }
        }
        [warm, cool, usize::from(max.saturating_sub(min))]
    };

    let first = draw(initial)?;
    assert_eq!([first.draws, first.instances, first.baked], [1, 2, 1]);
    assert_eq!(
        first.compacted, 1,
        "the offscreen instance fragments the draw"
    );
    assert_eq!(first.uploads, 1, "both atlas regions share the texture");
    let left = colors(&first, true);
    let right = colors(&first, false);
    assert!(left[0] > 3000 && right[1] > 3000, "{left:?} {right:?}");
    assert!(
        left[2] > 45,
        "the dedicated chart preserves the baked light and shadow gradient"
    );
    let cached = draw(initial)?;
    assert_eq!([cached.draws, cached.baked, cached.uploads], [0, 0, 1]);
    assert!(cached.view == first.view);

    let swapped = draw(State {
        swap: true,
        ..initial
    })?;
    assert!(colors(&swapped, true)[1] > 3000 && colors(&swapped, false)[0] > 3000);
    assert_eq!([swapped.draws, swapped.uploads], [1, 1]);
    let full_with_live = draw(State {
        live: 2.0,
        ..initial
    })?;
    assert!(
        full_with_live.view == first.view,
        "a diffuse bake replaces live diffuse on matte geometry"
    );
    let indirect = State {
        full: false,
        ..initial
    };
    let indirect_unlit = draw(indirect)?;
    let indirect_lit = draw(State {
        live: 2.0,
        ..indirect
    })?;
    let lit_pixels = indirect_lit
        .view
        .iter()
        .zip(&indirect_unlit.view)
        .filter(|(after, before)| **after > before.saturating_add(20))
        .count();
    assert!(
        lit_pixels > 5000,
        "an indirect bake keeps additional direct lighting"
    );

    let uncharted = State {
        chart: false,
        ..initial
    };
    let chartless = draw(uncharted)?;
    let no_bake = draw(State {
        baked: false,
        ..uncharted
    })?;
    assert_eq!(chartless.baked, 0);
    assert!(chartless.view == no_bake.view);
    let zero = draw(State {
        gain: 0.0,
        ..initial
    })?;
    assert_eq!(zero.baked, 0);

    // Compare below display clipping to isolate SSGI's lighting contribution.
    // Its linear intermediate resolves overbright MSAA samples before clamping.
    let combined = State {
        msaa: true,
        gain: 1.0,
        ..initial
    };
    let msaa = draw(combined)?;
    let with_ssgi = draw(State {
        ssgi: true,
        ..combined
    })?;
    assert_eq!(
        [with_ssgi.baked, with_ssgi.ssgi, with_ssgi.uploads],
        [1, 1, 1]
    );
    assert!(
        with_ssgi
            .view
            .iter()
            .zip(&msaa.view)
            .all(|(a, b)| a.abs_diff(*b) <= 1),
        "SSGI preserves the stored illumination of baked receivers"
    );
    eprintln!(
        "Shared atlas produced {} warm and {} cool pixels in one draw",
        left[0], right[1]
    );
    Ok(())
}

#[test]
fn screen_indirect_light_bounces_color_and_shares_the_opaque_pass() -> TestResult {
    use screen_indirect_ui::ScreenIndirectDemo;

    const NAME: &str = "examples/puzzle3d/screen_indirect.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ScreenIndirectDemo")?;
    let generated = ScreenIndirectDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;

    #[derive(Clone, Copy, Debug)]
    struct State {
        enabled: bool,
        warm: bool,
        strength: f64,
        occlusion: f64,
        ambient: f64,
        scale: f64,
        msaa: bool,
        ssr: bool,
        ssao: bool,
    }
    struct Frame {
        view: Vec<u8>,
        rays: [u64; 2],
        depth: u64,
        geometry: u64,
        ssao: u64,
    }
    let base = State {
        enabled: false,
        warm: true,
        strength: 3.0,
        occlusion: 0.0,
        ambient: 0.32,
        scale: 0.5,
        msaa: false,
        ssr: false,
        ssao: false,
    };
    let mut draw = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing screen indirect demo")?;
        app.set_global_light(state.enabled);
        app.set_warm_panel(state.warm);
        app.set_strength(state.strength);
        app.set_occlusion(state.occlusion);
        app.set_ambient_level(state.ambient);
        app.set_buffer_scale(state.scale);
        app.set_msaa(state.msaa);
        app.set_reflect_screen(state.ssr);
        app.set_ambient_occlusion(state.ssao);
        for (name, value) in [
            ("global-light", Value::Bool(state.enabled)),
            ("warm-panel", Value::Bool(state.warm)),
            ("strength", Value::Float(state.strength)),
            ("occlusion", Value::Float(state.occlusion)),
            ("ambient-level", Value::Float(state.ambient)),
            ("buffer-scale", Value::Float(state.scale)),
            ("msaa", Value::Bool(state.msaa)),
            ("reflect-screen", Value::Bool(state.ssr)),
            ("ambient-occlusion", Value::Bool(state.ssao)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        assert!(
            image.as_slice() == interpreted.render(&mut preview_gpu)?,
            "{state:?}"
        );
        let after = gpu.scene_draw_stats();
        Ok(Frame {
            view: xrack_assets::decode_png(&image)?.rgba[..600 * 340 * 4].to_vec(),
            rays: [
                after.screen_indirect_pixels - before.screen_indirect_pixels,
                after.screen_reflection_pixels - before.screen_reflection_pixels,
            ],
            depth: after.opaque_depth_captures - before.opaque_depth_captures,
            geometry: after.color_draws - before.color_draws,
            ssao: after.occlusion_evaluations - before.occlusion_evaluations,
        })
    };

    let warm_off = draw(base)?;
    let cool = State {
        warm: false,
        ..base
    };
    let cool_off = draw(cool)?;
    let warm_on = draw(State {
        enabled: true,
        ..base
    })?;
    let cached = draw(State {
        enabled: true,
        ..base
    })?;
    let cool_on = draw(State {
        enabled: true,
        ..cool
    })?;
    assert_eq!(warm_off.rays, [0, 0]);
    assert_eq!(warm_on.rays, [300 * 170, 0]);
    assert_eq!(warm_on.geometry, warm_off.geometry);
    assert_eq!([cached.rays[0], cached.rays[1], cached.geometry], [0; 3]);
    assert!(cached.view == warm_on.view);
    // Compare only receivers whose direct color is identical with either
    // emitter. A changed panel pixel alone cannot satisfy the assertion.
    let spill = warm_off
        .view
        .chunks_exact(4)
        .zip(cool_off.view.chunks_exact(4))
        .zip(
            warm_on
                .view
                .chunks_exact(4)
                .zip(cool_on.view.chunks_exact(4)),
        )
        .filter(|((old, other), (warm, cool))| {
            old == other
                && warm[0] > cool[0].saturating_add(3)
                && cool[2] > warm[2].saturating_add(3)
        })
        .count();
    assert!(spill > 100, "only {spill} receiver pixels changed hue");
    eprintln!("Screen-space bounce changed hue at {spill} receiver pixels");

    let ao_only = State {
        enabled: true,
        strength: 0.0,
        occlusion: 1.0,
        ..base
    };
    let ao = draw(ao_only)?;
    let darkened = ao
        .view
        .iter()
        .zip(&warm_off.view)
        .filter(|(a, b)| a.saturating_add(3) < **b)
        .count();
    assert!(darkened > 100);
    assert!(
        ao.view
            .iter()
            .zip(&warm_off.view)
            .all(|(a, b)| *a <= b.saturating_add(1))
    );
    let direct = draw(State {
        ambient: 0.0,
        ..base
    })?;
    let direct_with_ao = draw(State {
        ambient: 0.0,
        ..ao_only
    })?;
    assert!(
        direct
            .view
            .iter()
            .zip(&direct_with_ao.view)
            .all(|(a, b)| a.abs_diff(*b) <= 1),
        "occlusion must not attenuate direct light"
    );
    let zero = draw(State {
        enabled: true,
        strength: 0.0,
        ..base
    })?;
    assert_eq!(zero.rays, [0, 0]);
    let _ = draw(State {
        enabled: true,
        scale: 0.25,
        ..base
    })?;
    let combined = State {
        enabled: true,
        msaa: true,
        ssr: true,
        ..base
    };
    let both = draw(combined)?;
    let plain = draw(State {
        enabled: false,
        ssr: false,
        ..combined
    })?;
    assert_eq!(both.rays, [300 * 170; 2]);
    assert_eq!(both.depth, 1);
    assert_eq!(both.geometry, plain.geometry);
    assert!(both.view != plain.view);
    let quarter = draw(State {
        scale: 0.25,
        ..combined
    })?;
    assert_eq!(quarter.rays, [150 * 85, 300 * 170]);
    assert_eq!(quarter.depth, 1);
    let replacement = draw(State {
        ssao: true,
        ..ao_only
    })?;
    assert_eq!(replacement.ssao, 0);
    let separate = draw(State {
        enabled: true,
        ssao: true,
        ..base
    })?;
    assert_eq!(separate.ssao, 1);
    Ok(())
}

#[test]
fn screen_reflections_follow_visible_geometry_and_preserve_the_probe_fallback() -> TestResult {
    use screen_reflections_ui::ScreenReflectionsDemo;

    const NAME: &str = "examples/puzzle3d/screen_reflections.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ScreenReflectionsDemo")?;
    let generated = ScreenReflectionsDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;

    #[derive(Clone, Copy, Debug)]
    struct State {
        enabled: bool,
        visible: bool,
        studio: bool,
        offset: f64,
        cutoff: f64,
        scale: f64,
        msaa: bool,
        aa_scale: f64,
        glass: i64,
        weighted: bool,
    }
    let baseline = State {
        enabled: false,
        visible: true,
        studio: false,
        offset: -1.5,
        cutoff: 0.65,
        scale: 0.5,
        msaa: false,
        aa_scale: 1.0,
        glass: 0,
        weighted: false,
    };
    let mut draw = |state: State, pixels: u64| -> TestResult<(xrack_assets::Raster, u64)> {
        let mut app = weak.upgrade().ok_or("missing screen reflection demo")?;
        app.set_reflect_screen(state.enabled);
        app.set_show_source(state.visible);
        app.set_studio(state.studio);
        app.set_offset(state.offset);
        app.set_cutoff(state.cutoff);
        app.set_trace_scale(state.scale);
        app.set_msaa(state.msaa);
        app.set_aa_scale(state.aa_scale);
        app.set_glass_mode(state.glass);
        app.set_weighted(state.weighted);
        for (name, value) in [
            ("reflect-screen", Value::Bool(state.enabled)),
            ("show-source", Value::Bool(state.visible)),
            ("studio", Value::Bool(state.studio)),
            ("offset", Value::Float(state.offset)),
            ("cutoff", Value::Float(state.cutoff)),
            ("trace-scale", Value::Float(state.scale)),
            ("msaa", Value::Bool(state.msaa)),
            ("aa-scale", Value::Float(state.aa_scale)),
            ("glass-mode", Value::Int(state.glass)),
            ("weighted", Value::Bool(state.weighted)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png.as_slice() == interpreted.render(&mut preview_gpu)?,
            "{state:?}"
        );
        let after = gpu.scene_draw_stats();
        assert_eq!(
            after.screen_reflection_pixels - before.screen_reflection_pixels,
            pixels
        );
        assert_eq!(
            after.screen_reflection_resolves - before.screen_reflection_resolves,
            u64::from(pixels > 0)
        );
        let multisampling = state.msaa && state.aa_scale <= 1.0;
        assert_eq!(
            after.opaque_depth_captures - before.opaque_depth_captures,
            u64::from(state.glass == 3 || multisampling && pixels > 0),
            "{state:?}",
        );
        assert_eq!(
            after.volume_depth_captures - before.volume_depth_captures,
            u64::from(state.glass == 3),
            "{state:?}",
        );
        assert_eq!(
            after.hdr_resolves - before.hdr_resolves,
            u64::from(state.studio)
        );
        if state.glass >= 2 {
            assert_eq!(
                after.transmission_captures - before.transmission_captures,
                1
            );
        }
        if state.glass == 1 && state.weighted {
            assert_eq!(
                after.transparency_composites - before.transparency_composites,
                1
            );
        }
        Ok((
            xrack_assets::decode_png(&png)?,
            after.color_draws - before.color_draws,
        ))
    };
    let (plain, baseline_draws) = draw(baseline, 0)?;
    let enabled = State {
        enabled: true,
        ..baseline
    };
    let (reflected, draws) = draw(enabled, 300 * 170)?;
    assert_eq!(
        baseline_draws, draws,
        "the forward pass emits the reflection attributes"
    );
    let (cached, draws) = draw(enabled, 0)?;
    assert_eq!(draws, 0);
    assert!(cached.rgba == reflected.rgba);

    let red_centroid = |image: &xrack_assets::Raster| {
        let [width, _] = image.size.map(|value| value as usize);
        let mut count = 0;
        let mut sum = 0;
        for y in 195..252 {
            for x in 15..width - 15 {
                let start = 4 * (y * width + x);
                let [r, g, b, _] = [0, 1, 2, 3].map(|channel| image.rgba[start + channel]);
                if r > g.saturating_add(28) && r > b.saturating_add(23) {
                    count += 1;
                    sum += x;
                }
            }
        }
        (count, sum / count.max(1))
    };
    let (moved, _) = draw(
        State {
            offset: 0.55,
            ..enabled
        },
        300 * 170,
    )?;
    let (original_count, original_x) = red_centroid(&reflected);
    let (moved_count, moved_x) = red_centroid(&moved);
    assert!(
        original_count > 200 && moved_count > 200,
        "{original_count} and {moved_count}"
    );
    assert!(
        moved_x > original_x + 60,
        "reflection moved from {original_x} to {moved_x}"
    );
    assert!(red_centroid(&plain).0 < 5);
    eprintln!(
        "Visible reflected red pixels {original_count} → {moved_count}, centroid {original_x} → {moved_x}"
    );

    let rough = draw(
        State {
            cutoff: 0.1,
            ..enabled
        },
        0,
    )?
    .0;
    let area = 600 * 340 * 4;
    assert!(rough.rgba[..area] == plain.rgba[..area]);
    let _ = draw(
        State {
            scale: 0.25,
            ..enabled
        },
        150 * 85,
    )?;
    let _ = draw(
        State {
            aa_scale: 1.5,
            ..enabled
        },
        450 * 255,
    )?;
    let with_msaa = State {
        msaa: true,
        ..enabled
    };
    let (msaa_enabled, msaa_draws) = draw(with_msaa, 300 * 170)?;
    let (msaa_disabled, plain_msaa_draws) = draw(
        State {
            enabled: false,
            ..with_msaa
        },
        0,
    )?;
    assert_eq!(msaa_draws, plain_msaa_draws);
    assert!(red_centroid(&msaa_enabled).0 > 200);
    assert!(red_centroid(&msaa_disabled).0 < 5);

    // Reflection metadata only comes from the floor below this band.
    // Retain both the ordinary MSAA silhouette and its per-sample colors.
    let upper = 600 * 120 * 4;
    let gap = msaa_enabled.rgba[..upper]
        .iter()
        .zip(&msaa_disabled.rgba)
        .map(|(a, b)| a.abs_diff(*b))
        .max();
    assert!(
        gap.is_some_and(|difference| difference <= 2),
        "opaque MSAA color changed by {gap:?}"
    );
    let edge_changes = msaa_enabled.rgba[..upper]
        .chunks_exact(4)
        .zip(reflected.rgba.chunks_exact(4))
        .filter(|(msaa, single)| msaa[2].abs_diff(single[2]) > 12)
        .count();
    assert!(
        edge_changes > 20,
        "MSAA changed coverage at {edge_changes} edge pixels"
    );

    for msaa in [false, true] {
        for (glass, weighted) in [(1, false), (1, true), (2, false), (3, false), (3, true)] {
            let scene = State {
                glass,
                weighted,
                msaa,
                ..enabled
            };
            let (with_glass, geometry_draws) = draw(scene, 300 * 170)?;
            let (_, without_ssr_draws) = draw(
                State {
                    enabled: false,
                    ..scene
                },
                0,
            )?;
            assert!(with_glass.rgba[..area] != reflected.rgba[..area]);
            assert_eq!(geometry_draws, without_ssr_draws, "{scene:?}");
        }
    }

    let sky = State {
        studio: true,
        ..enabled
    };
    let (hdr_refl, _) = draw(sky, 300 * 170)?;
    let (hdr_msaa, _) = draw(State { msaa: true, ..sky }, 300 * 170)?;
    let (hdr_plain, _) = draw(
        State {
            enabled: false,
            ..sky
        },
        0,
    )?;
    let (hdr_msaa_plain, _) = draw(
        State {
            enabled: false,
            msaa: true,
            ..sky
        },
        0,
    )?;
    assert!(hdr_refl.rgba[..area] != hdr_plain.rgba[..area]);
    assert!(hdr_msaa.rgba[..area] != hdr_msaa_plain.rgba[..area]);
    let empty = State {
        visible: false,
        ..sky
    };
    let (fallback, _) = draw(empty, 300 * 170)?;
    let (probe, _) = draw(
        State {
            enabled: false,
            ..empty
        },
        0,
    )?;
    let difference = fallback.rgba[..area]
        .iter()
        .zip(&probe.rgba)
        .map(|(a, b)| a.abs_diff(*b))
        .max();
    assert!(
        difference.is_some_and(|gap| gap <= 1),
        "probe fallback: {difference:?}"
    );
    Ok(())
}

#[test]
fn motion_vectors_reproject_rigid_surfaces_and_discard_disocclusions() -> TestResult {
    use temporal_aa_ui::TemporalAaDemo;

    const NAME: &str = "examples/puzzle3d/temporal_aa.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("TemporalAaDemo")?;
    dynamic.set("motion-vectors", Value::Bool(true))?;
    let mut app = TemporalAaDemo::new();
    app.set_motion_vectors(true);
    let weak = app.as_weak();
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;
    let mut draw = |angle, scope, should_draw, history, pending| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing motion-vectors demo")?;
        app.set_angle(angle);
        dynamic.set("angle", Value::Float(angle))?;
        gpu.set_scene_observation(scope, false);
        preview_gpu.set_scene_observation(scope, false);
        let before = gpu.scene_draw_stats();
        let png = generated.render_animated(&mut gpu)?.to_vec();
        assert!(png.as_slice() == interpreted.render_animated(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        assert_eq!(
            after.motion_vector_draws > before.motion_vector_draws,
            should_draw
        );
        assert_eq!(
            after.temporal_reprojections - before.temporal_reprojections,
            u64::from(history)
        );
        assert_eq!(after.temporal_aa_blends, before.temporal_aa_blends);
        assert_eq!(after.progressive_aa_samples, before.progressive_aa_samples);
        assert_eq!(gpu.needs_redraw(), pending);
        assert_eq!(interpreted.next_frame_delay().is_some(), pending);
        Ok((
            xrack_assets::decode_png(&png)?,
            [gpu.scene_mesh_uploads(), gpu.scene_instance_uploads()],
        ))
    };
    let _ = draw(0.0, 1, true, false, true)?;
    for index in 1..8 {
        let _ = draw(0.0, 1, true, true, index < 7)?;
    }
    let _ = draw(0.0, 1, false, false, false)?;
    // A separate scope starts cold at the same subpixel phase. History must
    // contribute real color to the warm render, beyond just drawing vectors.
    let (warm, _) = draw(3.0, 1, true, true, true)?;
    let (cold, _) = draw(3.0, 2, true, false, true)?;
    let pixel = |image: &xrack_assets::Raster, [x, y]: [usize; 2]| -> [u8; 4] {
        let start = 4 * (y * image.size[0] as usize + x);
        std::array::from_fn(|channel| image.rgba[start + channel])
    };
    let viewport_changes = (40..280)
        .flat_map(|y| (390..743).map(move |x| [x, y]))
        .filter(|&at| pixel(&warm, at) != pixel(&cold, at))
        .count();
    assert!(
        viewport_changes > 5,
        "history changed only {viewport_changes} pixels"
    );

    let (moved, _) = draw(85.0, 1, true, true, true)?;
    let background = pixel(&moved, [15, 40]);
    let mut newly_visible = 0;
    for y in 40..280 {
        for x in 16..370 {
            let safely_background = (-2..=2).all(|dy| {
                (-2..=2).all(|dx| {
                    pixel(&moved, [(x as i32 + dx) as usize, (y as i32 + dy) as usize])
                        == background
                })
            });
            if safely_background && pixel(&warm, [x, y]) != background {
                newly_visible += 1;
                assert_eq!(
                    pixel(&moved, [x + 374, y]),
                    background,
                    "old silhouette at {x}, {y}"
                );
            }
        }
    }
    assert!(newly_visible > 100);
    let change_bar_motion = |enabled, scale| -> TestResult {
        let mut app = weak.upgrade().ok_or("missing motion-vectors demo")?;
        app.set_bar_motion(enabled);
        app.set_bar_motion_scale(scale);
        dynamic.set("bar-motion", Value::Bool(enabled))?;
        dynamic.set("bar-motion-scale", Value::Float(scale))?;
        Ok(())
    };
    let (masked, scaled) = {
        let mut warm_pose = |scope| -> TestResult<_> {
            for index in 0..8 {
                let _ = draw(0.0, scope, true, index != 0, index < 7)?;
            }
            draw(3.0, scope, true, true, true)
        };
        change_bar_motion(false, 1.0)?;
        let masked = warm_pose(3)?;
        change_bar_motion(true, 0.0)?;
        let scaled = warm_pose(4)?;
        (masked.0, scaled)
    };
    change_bar_motion(true, 0.5)?;
    let (_, uploads) = draw(3.0, 4, true, true, true)?;
    let (scaled, previous_uploads) = scaled;
    assert_eq!(
        uploads, previous_uploads,
        "motion metadata reuses color buffers"
    );

    let changed_pixels = |a, b, xs: std::ops::Range<usize>| {
        (40..280)
            .flat_map(|y| xs.clone().map(move |x| [x, y]))
            .filter(|&at| pixel(a, at) != pixel(b, at))
            .count()
    };
    assert!(changed_pixels(&warm, &cold, 390..552) > 5);
    assert_eq!(
        changed_pixels(&masked, &cold, 390..552),
        0,
        "disabled coverage"
    );
    assert!(changed_pixels(&scaled, &warm, 390..552) > 5);
    assert_eq!(
        changed_pixels(&masked, &warm, 600..743),
        0,
        "unaffected motion"
    );
    assert_eq!(changed_pixels(&scaled, &warm, 600..743), 0);
    assert!(changed_pixels(&warm, &cold, 600..743) > 5);

    eprintln!(
        "Reprojected {viewport_changes} pixels and rejected {newly_visible} disoccluded pixels"
    );
    Ok(())
}

#[test]
fn motion_vectors_track_gltf_skin_and_morph_poses() -> TestResult {
    use scene3d_model_ui::{MorphScene, SkinnedScene};
    use xrack_layout::Interaction;

    const NAME: &str = "tests/scene3d_model.xr";
    let preview = xrack_compiler::prepare_preview(
        include_str!("scene3d_model.xr"),
        NAME,
        Default::default(),
    )?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let mut gpu = renderer()?;
    let mut preview_gpu = renderer()?;
    let input = Interaction::default();

    fn enable_motion(node: &mut Node) -> bool {
        if node.kind == "SceneView3D" {
            node.properties.insert("temporal-aa", Value::Bool(true));
            node.properties
                .insert("temporal-aa-mode", Value::String("motion-vectors".into()));
            return true;
        }
        node.children.iter_mut().any(enable_motion)
    }
    let mut draw = |scope,
                    mut aot: Node,
                    mut interpreted: Node,
                    history,
                    [width, height]: [u32; 2]|
     -> TestResult<_> {
        assert!(enable_motion(&mut aot));
        assert!(enable_motion(&mut interpreted));
        gpu.set_scene_observation(scope, false);
        preview_gpu.set_scene_observation(scope, false);
        let before = gpu.scene_draw_stats();
        let rendered = gpu.snapshot(&aot, width, height, 1.0, &input)?;
        let dynamic = preview_gpu.snapshot(&interpreted, width, height, 1.0, &input)?;
        assert!(rendered.rgba == dynamic.rgba);
        let after = gpu.scene_draw_stats();
        assert!(after.motion_vector_draws > before.motion_vector_draws);
        assert_eq!(
            after.temporal_reprojections - before.temporal_reprojections,
            u64::from(history)
        );
        Ok(rendered.rgba)
    };

    let mut skinned = SkinnedScene::new();
    let mut dynamic_skin = program.create("SkinnedScene")?;
    let rest = draw(
        1,
        skinned.render(),
        dynamic_skin.render(),
        false,
        [200, 200],
    )?;
    skinned.set_time(1.0);
    dynamic_skin.set("time", Value::Float(1.0))?;
    let posed = draw(1, skinned.render(), dynamic_skin.render(), true, [200, 200])?;
    assert!(rest != posed);

    let mut morph = MorphScene::new();
    let mut dynamic_morph = program.create("MorphScene")?;
    morph.set_clip("Morph".into());
    dynamic_morph.set("clip", Value::String("Morph".into()))?;
    let rest = draw(2, morph.render(), dynamic_morph.render(), false, [200, 200])?;
    morph.set_time(0.5);
    dynamic_morph.set("time", Value::Float(0.5))?;
    let posed = draw(2, morph.render(), dynamic_morph.render(), true, [200, 200])?;
    assert!(rest != posed);

    const LOD: &str = "examples/puzzle3d/lod.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(LOD)?, LOD, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, LOD, None)?;
    let mut combined = lod_ui::LodDemo::new();
    let mut dynamic = program.create("LodDemo")?;
    combined.set_automatic(true);
    combined.set_deformation(true);
    dynamic.set("automatic", Value::Bool(true))?;
    dynamic.set("deformation", Value::Bool(true))?;
    combined.set_shadows(true);
    dynamic.set("shadows", Value::Bool(true))?;
    let rest = draw(3, combined.render(), dynamic.render(), false, [480, 280])?;
    combined.set_time(0.65);
    dynamic.set("time", Value::Float(0.65))?;
    let moved = draw(3, combined.render(), dynamic.render(), true, [480, 280])?;
    assert!(rest != moved);

    const GLASS: &str = "examples/puzzle3d/transmission.xr";
    let preview = xrack_compiler::prepare_preview(
        &std::fs::read_to_string(GLASS)?,
        GLASS,
        Default::default(),
    )?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, GLASS, None)?;
    let mut glass = transmission_ui::TransmissionDemo::new();
    let mut dynamic = program.create("TransmissionDemo")?;
    glass.set_overlay(true);
    glass.set_optical_overlay(true);
    dynamic.set("overlay", Value::Bool(true))?;
    dynamic.set("optical-overlay", Value::Bool(true))?;
    let _ = draw(4, glass.render(), dynamic.render(), false, [480, 294])?;
    let _ = draw(4, glass.render(), dynamic.render(), true, [480, 294])?;
    Ok(())
}

#[test]
fn temporal_history_clears_after_aborted_hdr_supersampled_frame() -> TestResult {
    use xrack_layout::Interaction;
    use xrack_runtime::Length;

    let mut app = temporal_aa_ui::TemporalAaDemo::new();
    app.set_progressive(false);
    let mut scene = app.render();
    let view = scene
        .children
        .iter_mut()
        .find(|node| node.id == Some("filtered"))
        .ok_or("missing temporal comparison viewport")?;
    view.properties.insert("scene-hdr", Value::Bool(true));
    view.properties.insert("supersampling", Value::Float(1.5));
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = Interaction::default();
    let first = gpu.snapshot(&scene, 760, 352, 1.0, &input)?;
    let stats = gpu.scene_draw_stats();
    assert_eq!(stats.hdr_resolves, 1);
    assert_eq!(stats.supersample_resolves, 1);
    assert_eq!(stats.temporal_aa_samples, 1);
    assert_eq!(stats.temporal_aa_blends, 0);

    let registration = xrack_renderer_femtovg::register_gpu("abort-temporal-aa", |_| {
        Err("interrupted temporal frame".into())
    })?;
    let mut broken = Node::new("CustomRender", Some("abort"));
    for (name, value) in [
        ("renderer", Value::String("abort-temporal-aa".into())),
        ("width", Value::Length(Length(24.0))),
        ("height", Value::Length(Length(24.0))),
        ("z", Value::Float(200.0)),
    ] {
        broken.properties.insert(name, value);
    }
    let mut failing = scene.clone();
    failing.children.push(broken);
    let error = gpu
        .snapshot(&failing, 760, 352, 1.0, &input)
        .err()
        .ok_or("expected failed temporal frame")?;
    assert!(error.contains("interrupted temporal frame"));
    assert_eq!(gpu.scene_draw_stats().temporal_aa_blends, 1);
    drop(registration);

    let restarted = gpu.snapshot(&scene, 760, 352, 1.0, &input)?;
    assert!(restarted.rgba == first.rgba);
    assert_eq!(gpu.scene_draw_stats().temporal_aa_blends, 1);
    assert!(gpu.needs_redraw());
    let _ = gpu.snapshot(&scene, 760, 352, 1.0, &input)?;
    assert_eq!(gpu.scene_draw_stats().temporal_aa_blends, 2);
    assert!(!gpu.needs_redraw());
    Ok(())
}

fn exercise(component: Box<dyn Component>, renderer: &mut Renderer) -> TestResult<Vec<Vec<u8>>> {
    let mut scene = PreviewScene::new(component);
    let mut frames = vec![scene.render(renderer)?.to_vec()];
    let (view, layout) = inspect(&scene)?;
    let Some(Value::SceneObjects(objects)) = view.properties.get("model") else {
        return Err("missing prepared scene objects".into());
    };
    for id in ["amber", "exit"] {
        assert!(
            objects
                .iter()
                .any(|object| object.id == id && matches!(object.mesh, Mesh3D::Imported(_)))
        );
    }
    let amber = objects
        .iter()
        .find(|object| object.id == "amber")
        .ok_or("missing amber mesh")?;
    let exit = objects
        .iter()
        .find(|object| object.id == "exit")
        .ok_or("missing exit mesh")?;
    assert!(amber.mesh.tangents().iter().any(|tangent| tangent[3] != 0));
    assert!(
        exit.mesh.tangents().is_empty(),
        "materials without normal maps skip tangent data"
    );
    let original_yaw = view.number("camera-yaw").ok_or("missing camera yaw")?;
    let bounds = layout.bounds;
    assert!(bounds.width > 200.0 && bounds.height > 200.0, "{bounds:?}");

    let contact = PointerContact {
        id: 7,
        primary: true,
        event: Default::default(),
    };
    let first = Point {
        x: bounds.x + 60.0,
        y: bounds.y + 75.0,
    };
    let second = Point {
        x: first.x + 42.0,
        y: first.y - 16.0,
    };
    assert!(scene.press_contact(contact, first, false));
    assert!(scene.move_contact(contact, second));
    assert!(scene.finish_contact(contact, second, false, false));
    frames.push(scene.render(renderer)?.to_vec());
    assert_ne!(frames[0], frames[1]);

    let (view, layout) = inspect(&scene)?;
    let yaw = view
        .number("camera-yaw")
        .ok_or("missing updated camera yaw")?;
    assert_ne!(yaw, original_yaw);
    let bounds = layout.bounds;
    let camera = SceneCamera::new(
        yaw,
        view.number("camera-pitch").ok_or("missing camera pitch")?,
        view.number("camera-distance")
            .ok_or("missing camera distance")?,
        [bounds.width.into(), bounds.height.into()],
    )
    .ok_or("invalid preview camera")?;
    let [x, y, _, w] = camera.clip_position([-1.0, 0.15, 0.0]);
    let amber = Point {
        x: bounds.x + bounds.width * (x / w + 1.0) * 0.5,
        y: bounds.y + bounds.height * (1.0 - y / w) * 0.5,
    };
    assert!(scene.press_contact(contact, amber, false));
    assert!(scene.finish_contact(contact, amber, true, false));
    frames.push(scene.render(renderer)?.to_vec());
    assert_eq!(inspect(&scene)?.0.text("selected-object"), Some("amber"));
    assert_ne!(frames[1], frames[2]);
    Ok(frames)
}

#[test]
fn puzzle_preview_pixels_orbit_and_depth_pick_match_both_backends() -> TestResult {
    let filename = "examples/puzzle3d/puzzle.xr";
    let source = std::fs::read_to_string(filename)?;
    let prepare = |source: &str| -> TestResult<xrack_interpreter::Program> {
        let mut preview = xrack_compiler::prepare_preview(source, filename, Default::default())?;
        let translations = preview.take_translation_catalog();
        let (program, paths) = xrack_interpreter::Program::from_module_with_file_resources(
            &preview.module,
            filename,
            translations,
        )?;
        assert!(paths.iter().any(|path| path.ends_with("models/exit.bin")));
        assert!(paths.iter().any(|path| path.ends_with("models/exit.png")));
        Ok(program)
    };
    let program = prepare(&source)?;
    let preview = xrack_compiler::prepare_preview(&source, filename, Default::default())?;
    let directory = std::path::Path::new(filename)
        .parent()
        .ok_or("missing example directory")?;
    let declared = [
        directory.join("models/amber.glb"),
        directory.join("models/exit.gltf"),
        directory.join("models/glass.gltf"),
        directory.join("models/studio.hdr"),
    ];
    let error = xrack_compiler::check_resources(&preview.module, filename, Some(&declared))
        .err()
        .ok_or("accepted an undeclared glTF buffer")?;
    assert!(error.to_string().contains("not declared"), "{error}");
    let declared_with_buffer = [
        declared[0].clone(),
        declared[1].clone(),
        declared[2].clone(),
        declared[3].clone(),
        directory.join("models/exit.bin"),
    ];
    let error =
        xrack_compiler::check_resources(&preview.module, filename, Some(&declared_with_buffer))
            .err()
            .ok_or("accepted an undeclared glTF texture")?;
    assert!(
        error.to_string().contains("exit.png") && error.to_string().contains("not declared"),
        "{error}"
    );
    let aliased = source.replace(
        "mesh: \"primitive:sphere\", y: 0.14",
        "mesh: @mesh-url(\"./models/amber.glb\"), y: 0.14",
    );
    assert_ne!(aliased, source);
    let packed = xrack_compiler::compile_preview(&aliased, filename, Default::default())?;
    let packed = packed.rust.to_string();
    assert_eq!(packed.matches("Mesh3D :: from_packed").count(), 3);
    assert_eq!(packed.matches("MeshImage :: from_packed").count(), 3);
    let gpu = pollster::block_on(xrack_renderer_femtovg::RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let aot = exercise(
        Box::new(Puzzle3D::new()),
        &mut Renderer::with_device(gpu.clone())?,
    )?;
    let dynamic = exercise(
        Box::new(program.create("Puzzle3D")?),
        &mut Renderer::with_device(gpu.clone())?,
    )?;
    assert_eq!(aot, dynamic);
    let mut generated = Puzzle3D::new();
    let interpreted = program.create("Puzzle3D")?;
    generated.set_amber_on(true);
    generated.set_blue_on(true);
    interpreted.set("amber-on", Value::Bool(true))?;
    interpreted.set("blue-on", Value::Bool(true))?;
    let mut renderer = Renderer::with_device(gpu)?;
    let relit_source = source.replace("light-yaw: 42;", "light-yaw: -58;");
    let relit = prepare(&relit_source)?.create("Puzzle3D")?;
    let mut relit = PreviewScene::new(Box::new(relit));
    assert_ne!(aot[0], relit.render(&mut renderer)?);

    let mut opened_aot = PreviewScene::new(Box::new(generated));
    let mut opened_dynamic = PreviewScene::new(Box::new(interpreted));
    let opened_image = opened_aot.render(&mut renderer)?.to_vec();
    assert_eq!(opened_image, opened_dynamic.render(&mut renderer)?);
    assert_ne!(aot[0], opened_image);

    let mut overview_aot = Puzzle3D::new();
    overview_aot.set_map_view(true);
    let overview_runtime = program.create("Puzzle3D")?;
    overview_runtime.set("map-view", Value::Bool(true))?;
    let mut overview_aot = PreviewScene::new(Box::new(overview_aot));
    let mut overview_runtime = PreviewScene::new(Box::new(overview_runtime));
    let overview = overview_aot.render(&mut renderer)?.to_vec();
    assert!(overview == overview_runtime.render(&mut renderer)?);
    assert_ne!(overview, aot[0]);
    let (view, layout) = inspect(&overview_aot)?;
    let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
        return Err("missing overview scene".into());
    };
    let bounds = layout.bounds;
    let camera = model
        .camera("overview", [bounds.width.into(), bounds.height.into()])
        .ok_or("missing overview camera")?;
    let [x, y, _, w] = camera.clip_position([1.0, 0.15, 0.0]);
    let blue = Point {
        x: bounds.x + bounds.width * (x / w + 1.0) * 0.5,
        y: bounds.y + bounds.height * (1.0 - y / w) * 0.5,
    };
    let contact = PointerContact {
        id: 8,
        primary: true,
        event: Default::default(),
    };
    for scene in [&mut overview_aot, &mut overview_runtime] {
        assert!(scene.press_contact(contact, blue, false));
        assert!(scene.finish_contact(contact, blue, true, false));
    }
    let selected = overview_aot.render(&mut renderer)?.to_vec();
    assert!(selected == overview_runtime.render(&mut renderer)?);
    assert_eq!(
        inspect(&overview_aot)?.0.text("selected-object"),
        Some("blue")
    );
    assert_eq!(
        inspect(&overview_runtime)?.0.text("selected-object"),
        Some("blue")
    );

    let filename = "tests/scene3d_model.xr";
    let preview = xrack_compiler::prepare_preview(
        include_str!("scene3d_model.xr"),
        filename,
        Default::default(),
    )?;
    let (fixture, inputs) = xrack_interpreter::Program::from_module_with_file_resources(
        &preview.module,
        filename,
        None,
    )?;
    assert!(
        inputs
            .iter()
            .any(|path| path.ends_with("models/studio.hdr"))
    );
    exercise_live_model(&mut renderer, &fixture)?;
    exercise_masked_material(&mut renderer, &fixture)?;
    exercise_blended_material(&mut renderer, &fixture)?;
    exercise_material_sides(&mut renderer, &fixture)?;
    exercise_pbr_materials(&mut renderer, &fixture)?;
    exercise_normal_maps(&mut renderer, &fixture)?;
    exercise_scene_lights(&mut renderer, &fixture)?;
    exercise_shadows(&mut renderer, &fixture)?;
    exercise_point_shadow_faces(&mut renderer, &fixture)?;
    exercise_directional_cascades(&mut renderer, &fixture)?;
    exercise_environment(&mut renderer, &fixture)?;
    exercise_hierarchy(&mut renderer, &fixture)?;
    exercise_named_cameras(&mut renderer, &fixture)?;
    exercise_mounted_lights(&mut renderer, &fixture)?;
    exercise_gltf_animation(&mut renderer, &fixture)?;
    exercise_skinning(&mut renderer, &fixture)?;
    exercise_morph_targets(&mut renderer, &fixture)?;
    exercise_morph_overrides(&mut renderer, &fixture)?;
    exercise_hit_lists(&mut renderer, &fixture)?;
    exercise_frustum_culling(&mut renderer, &fixture)?;
    exercise_posed_culling(&mut renderer, &fixture)?;
    exercise_lod(&mut renderer)
}

fn exercise_live_model(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::SceneModelTest;

    let interpreted = fixture.create("SceneModelTest")?;
    let mut generated = SceneModelTest::new();
    let generated_model = VecModel::from(generated.get_actors());
    generated.bind_model_actors(generated_model.clone());
    let interpreted_model = interpreted.record_model("actors")?;
    interpreted.set("actors", Value::RecordList(interpreted_model.clone()))?;
    assert_eq!(generated.get_hit(), "orb");
    assert_eq!(interpreted.get("hit")?, Value::String("orb".into()));

    let generated_handle = generated.as_weak();
    let mut generated_scene = PreviewScene::new(Box::new(generated));
    let mut interpreted_scene = PreviewScene::new(Box::new(interpreted.clone()));
    let first = generated_scene.render(renderer)?.to_vec();
    assert_eq!(first, interpreted_scene.render(renderer)?);

    let mut generated_orb = generated_model.row(0).ok_or("missing generated row")?;
    generated_orb.x = 6.0;
    let interpreted_orb = interpreted_model
        .model()
        .row(0)
        .ok_or("missing interpreted row")?;
    let interpreted_orb = interpreted_orb
        .with_field(&["x".into()], Value::Float(6.0))
        .ok_or("missing object coordinate")?;
    assert!(generated_model.set_row(0, generated_orb));
    assert!(interpreted_model.model().set_row(0, interpreted_orb));

    let moved = generated_scene.render(renderer)?.to_vec();
    assert_ne!(first, moved);
    assert_eq!(moved, interpreted_scene.render(renderer)?);
    let generated = generated_handle
        .upgrade()
        .ok_or("missing generated component")?;
    assert_eq!(generated.get_hit(), "");
    assert_eq!(interpreted.get("hit")?, Value::String(String::new()));
    Ok(())
}

fn exercise_masked_material(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::MaterialScene;

    let aot = MaterialScene::new();
    let interpreted = fixture.create("MaterialScene")?;
    assert_eq!(aot.get_left_hit(), "mask");
    assert_eq!(aot.get_right_hit(), "back");
    assert_eq!(interpreted.get("left-hit")?, Value::String("mask".into()));
    assert_eq!(interpreted.get("right-hit")?, Value::String("back".into()));
    let solid = aot.get_solid_texel();
    let hole = aot.get_transparent_texel();
    assert_eq!(interpreted.get("solid-texel")?, Value::Hit3D(solid.clone()));
    assert_eq!(
        interpreted.get("transparent-texel")?,
        Value::Hit3D(hole.clone())
    );
    assert!(solid.valid && hole.valid);
    assert_eq!(solid.object_id, "mask");
    assert_eq!(hole.object_id, "back");
    assert!(solid.u < 0.5 && (solid.v - 0.5).abs() < 1e-4, "{solid:?}");
    assert!(solid.normal.z > 0.99, "{solid:?}");
    assert!((solid.local_position.x * 4.0 - solid.position.x).abs() < 1e-5);
    assert!(hole.distance > solid.distance);
    assert_eq!(aot.get_world_x(), solid.position.x);
    assert_eq!(aot.get_normal_z(), solid.normal.z);
    let mut aot = PreviewScene::new(Box::new(aot));
    let mut dynamic = PreviewScene::new(Box::new(interpreted));
    let aot_pixels = aot.render(renderer)?.to_vec();
    assert_eq!(aot_pixels, dynamic.render(renderer)?);
    let pixels = xrack_assets::decode_png(&aot_pixels)?;
    let [width, height] = pixels.size.map(|value| value as usize);
    let pixel = |x: usize| {
        let offset = (width * (height / 2) + x * width / 200) * 4;
        &pixels.rgba[offset..offset + 4]
    };
    let left = pixel(78);
    let right = pixel(122);
    assert!(
        u16::from(left[1]) > u16::from(left[0]) + 50,
        "left texture: {left:?}"
    );
    assert!(
        u16::from(right[0]) > u16::from(right[1]) + 50,
        "right cutout: {right:?}"
    );
    Ok(())
}

fn scene_pixel(image: &xrack_assets::Raster, [x, y]: [usize; 2]) -> [u8; 4] {
    let [width, height] = image.size.map(|value| value as usize);
    let offset = (width * (y * height / 200) + x * width / 200) * 4;
    std::array::from_fn(|i| image.rgba[offset + i])
}

fn exercise_blended_material(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::BlendedScene;

    for rear in [false, true] {
        let mut generated = BlendedScene::new();
        let interpreted = fixture.create("BlendedScene")?;
        generated.set_rear(rear);
        interpreted.set("rear", Value::Bool(rear))?;
        assert_eq!(generated.get_center_hit(), "glass");
        assert_eq!(generated.get_left_hit(), "occluder");
        assert_eq!(
            interpreted.get("center-hit")?,
            Value::String("glass".into())
        );
        assert_eq!(
            interpreted.get("left-hit")?,
            Value::String("occluder".into())
        );
        let mut generated = PreviewScene::new(Box::new(generated));
        let mut interpreted = PreviewScene::new(Box::new(interpreted));
        let bytes = generated.render(renderer)?.to_vec();
        assert_eq!(bytes, interpreted.render(renderer)?);
        let image = xrack_assets::decode_png(&bytes)?;
        // The layers compose in linear light. Orbiting swaps their depth order.
        let expected = if rear {
            [188, 137, 137, 255]
        } else {
            [137, 188, 137, 255]
        };
        let center = scene_pixel(&image, [100, 100]);
        for (actual, expected) in center.into_iter().zip(expected) {
            assert!(actual.abs_diff(expected) <= 2, "center pixel {center:?}");
        }
        assert_eq!(scene_pixel(&image, [70, 100]), [0, 0, 255, 255]);
        assert_eq!(scene_pixel(&image, [0, 0]), [32, 42, 56, 255]);
    }
    Ok(())
}

fn exercise_material_sides(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::SidedScene;

    let mut front_double = [0; 4];
    for rear in [false, true] {
        let mut generated = SidedScene::new();
        let interpreted = fixture.create("SidedScene")?;
        generated.set_rear(rear);
        interpreted.set("rear", Value::Bool(rear))?;
        let right_hit = if rear { "" } else { "panels" };
        assert_eq!(generated.get_left_hit(), "panels");
        assert_eq!(generated.get_right_hit(), right_hit);
        assert_eq!(interpreted.get("left-hit")?, Value::String("panels".into()));
        assert_eq!(
            interpreted.get("right-hit")?,
            Value::String(right_hit.into())
        );
        let mut generated = PreviewScene::new(Box::new(generated));
        let mut interpreted = PreviewScene::new(Box::new(interpreted));
        let bytes = generated.render(renderer)?.to_vec();
        assert_eq!(bytes, interpreted.render(renderer)?);
        let image = xrack_assets::decode_png(&bytes)?;
        let (left, right) = (
            scene_pixel(&image, [69, 100]),
            scene_pixel(&image, [131, 100]),
        );
        if rear {
            for (actual, expected) in left.into_iter().zip(front_double) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "back-face normal should face the light: {left:?}"
                );
            }
            assert_eq!(
                right,
                [32, 42, 56, 255],
                "single-sided surface should be culled"
            );
        } else {
            assert!(
                left[0] > 225 && left[1] < 45,
                "mirrored glTF winding should be preserved: {left:?}"
            );
            assert!(
                right[1] > 225 && right[0] < 45,
                "double-sided front lighting: {right:?}"
            );
            front_double = right;
        }
    }
    Ok(())
}

fn exercise_pbr_materials(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::PbrScene;

    for lamp in 0..3 {
        let mut generated = PbrScene::new();
        let interpreted = fixture.create("PbrScene")?;
        generated.set_lamp(lamp);
        interpreted.set("lamp", Value::Int(lamp))?;
        let mut generated = PreviewScene::new(Box::new(generated));
        let mut interpreted = PreviewScene::new(Box::new(interpreted));
        let bytes = generated.render(renderer)?.to_vec();
        assert_eq!(bytes, interpreted.render(renderer)?);
        let image = xrack_assets::decode_png(&bytes)?;
        match lamp {
            0 => {
                // The symmetric dielectric panels differ only in sampled roughness.
                let smooth = scene_pixel(&image, [52, 68]);
                let rough = scene_pixel(&image, [148, 68]);
                assert!(
                    u16::from(smooth[0]) > u16::from(rough[0]) + 5,
                    "{smooth:?} vs {rough:?}"
                );
            }
            1 => {
                let metal = scene_pixel(&image, [100, 68]);
                let dielectric = scene_pixel(&image, [148, 68]);
                assert!(
                    u16::from(dielectric[0]) > u16::from(metal[0]) + 15,
                    "{dielectric:?} vs {metal:?}"
                );
            }
            _ => {
                // No direct light. Occlusion attenuates ambient and emission remains visible.
                assert_eq!(scene_pixel(&image, [52, 132]), [0, 0, 0, 255]);
                let partly_occluded = scene_pixel(&image, [100, 132]);
                assert!(
                    (74..=82).contains(&partly_occluded[0]),
                    "{partly_occluded:?}"
                );
                let emissive = scene_pixel(&image, [148, 132]);
                assert!(
                    emissive[0].abs_diff(128) <= 1 && emissive[1..] == [0, 0, 255],
                    "{emissive:?}"
                );
            }
        }
    }
    Ok(())
}

fn exercise_normal_maps(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::NormalMapScene;

    for rear in [false, true] {
        let mut aot = NormalMapScene::new();
        let interpreted = fixture.create("NormalMapScene")?;
        aot.set_rear(rear);
        interpreted.set("rear", Value::Bool(rear))?;
        assert_eq!(aot.get_top_hit(), "tiles");
        assert_eq!(interpreted.get("top-hit")?, Value::String("tiles".into()));
        let mut aot = PreviewScene::new(Box::new(aot));
        let mut dynamic = PreviewScene::new(Box::new(interpreted));
        let bytes = aot.render(renderer)?.to_vec();
        assert_eq!(bytes, dynamic.render(renderer)?);
        let image = xrack_assets::decode_png(&bytes)?;
        let mapped = [52, 100, 148].map(|x| scene_pixel(&image, [x, 68])[0]);
        let flat = scene_pixel(&image, [100, 132])[0];
        let mirrored_explicit = scene_pixel(&image, [if rear { 148 } else { 52 }, 132])[0];
        // Both authored and MikkTSpace-generated frames should keep the normal
        // pointed into the light, including in the two mirrored nodes.
        for light in mapped.into_iter().chain([mirrored_explicit]) {
            assert!(
                u16::from(light) > u16::from(flat) + 12,
                "mapped {light}, plain {flat}, rear {rear}"
            );
            assert!(
                light.abs_diff(mapped[1]) <= 6,
                "mapped {mapped:?}, mirrored authored {mirrored_explicit}, rear {rear}"
            );
        }
    }
    Ok(())
}

fn exercise_scene_lights(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::LightsScene;

    let aot = LightsScene::new();
    let generated = aot.as_weak();
    let interpreted = fixture.create("LightsScene")?;
    let mut aot = PreviewScene::new(Box::new(aot));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let mut last_aot: Option<Node> = None;
    let mut last_interpreted: Option<Node> = None;
    let mut frame =
        |mode: i64, distance: f64, range: f64, ambient: f64| -> TestResult<xrack_assets::Raster> {
            let mut app = generated.upgrade().ok_or("missing generated light scene")?;
            app.set_mode(mode);
            app.set_point_distance(distance);
            app.set_point_range(range);
            app.set_ambient(ambient);
            interpreted.set("mode", Value::Int(mode))?;
            interpreted.set("point-distance", Value::Float(distance))?;
            interpreted.set("point-range", Value::Float(range))?;
            interpreted.set("ambient", Value::Float(ambient))?;
            let image = aot.render(renderer)?.to_vec();
            assert_eq!(image, dynamic.render(renderer)?);
            let aot_frame = aot.inspect_frame().ok_or("missing AOT light frame")?.0;
            let interpreter_frame = dynamic
                .inspect_frame()
                .ok_or("missing interpreter light frame")?
                .0;
            for (previous, next) in [
                (&mut last_aot, aot_frame),
                (&mut last_interpreted, interpreter_frame),
            ] {
                if let Some(previous) = previous {
                    assert!(
                        previous.same_frame_inputs(next),
                        "light update should preserve layout inputs"
                    );
                }
                *previous = Some(next.clone());
            }
            Ok(xrack_assets::decode_png(&image)?)
        };

    let off = frame(0, 2.0, 2.6, 0.0)?;
    let directional = frame(1, 2.0, 2.6, 0.0)?;
    let point = frame(2, 2.0, 2.6, 0.0)?;
    let spot = frame(3, 2.0, 2.6, 0.0)?;
    let combined = frame(4, 2.0, 2.6, 0.0)?;
    let left = [61, 100];
    let spot_center = [139, 100];
    let spot_edge = [154, 100];
    let spot_outside = [174, 100];
    assert_eq!(
        scene_pixel(&off, left),
        [0, 0, 0, 255],
        "explicit empty light list"
    );
    assert!(scene_pixel(&directional, left)[0] > 80);
    assert!(scene_pixel(&point, left)[1] > 90);
    assert_eq!(scene_pixel(&point, spot_center)[1], 0, "point range cutoff");
    let bright = scene_pixel(&spot, spot_center)[2];
    let edge = scene_pixel(&spot, spot_edge)[2];
    assert!(
        bright > 100 && edge > 35 && u16::from(edge) + 12 < u16::from(bright),
        "spot core {bright}, fade {edge}"
    );
    assert_eq!(scene_pixel(&spot, spot_outside)[2], 0, "spot cone cutoff");
    for position in [left, spot_center, spot_edge] {
        let colors = [
            scene_pixel(&directional, position)[0],
            scene_pixel(&point, position)[1],
            scene_pixel(&spot, position)[2],
        ];
        let combined = scene_pixel(&combined, position);
        assert!(
            colors
                .into_iter()
                .zip(combined)
                .all(|(a, b)| a.abs_diff(b) <= 1),
            "{combined:?}"
        );
    }
    let near = scene_pixel(&frame(2, 2.0, 0.0, 0.0)?, left)[1];
    let far = scene_pixel(&frame(2, 4.0, 0.0, 0.0)?, left)[1];
    let green = |channel| {
        xrack_runtime::scene3d::linear_color(xrack_runtime::Color((u32::from(channel) << 16) | 255))
            [1]
    };
    assert!(
        (green(far) / green(near) - 0.25).abs() < 0.025,
        "inverse-square light: {near} → {far}"
    );
    let ambient = scene_pixel(&frame(0, 2.0, 0.0, 1.0)?, left);
    assert!(
        ambient[0] > 20 && ambient[1] > ambient[0] + 30 && ambient[1] == ambient[2],
        "{ambient:?}"
    );
    Ok(())
}

fn exercise_shadows(renderer: &mut Renderer, fixture: &xrack_interpreter::Program) -> TestResult {
    use scene3d_model_ui::ShadowScene;

    let generated = ShadowScene::new();
    let weak = generated.as_weak();
    let interpreted = fixture.create("ShadowScene")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let mut frame =
        |enabled: bool, mode: i64, orbit: f64| -> TestResult<(xrack_assets::Raster, u64)> {
            let mut aot_instance = weak.upgrade().ok_or("missing generated shadow scene")?;
            aot_instance.set_enabled(enabled);
            aot_instance.set_mode(mode);
            aot_instance.set_orbit(orbit);
            interpreted.set("enabled", Value::Bool(enabled))?;
            interpreted.set("mode", Value::Int(mode))?;
            interpreted.set("orbit", Value::Float(orbit))?;
            let image = aot.render(renderer)?.to_vec();
            assert_eq!(image, dynamic.render(renderer)?, "shadow mode {mode}");
            Ok((
                xrack_assets::decode_png(&image)?,
                renderer.scene_shadow_passes(),
            ))
        };

    let midpoint = [100, 100];
    let (without, before) = frame(false, 0, 0.0)?;
    let (with, baked) = frame(true, 0, 0.0)?;
    assert_eq!(
        baked,
        before + 1,
        "the same scene reuses its map across hosts"
    );
    let lit = scene_pixel(&without, midpoint)[0];
    let occluded = scene_pixel(&with, midpoint)[0];
    assert!(
        lit > 140 && occluded < 80,
        "directional: {lit} → {occluded}"
    );
    assert_eq!(
        scene_pixel(&without, [132, 100]),
        scene_pixel(&with, [132, 100])
    );
    let (orbited, after_orbit) = frame(true, 0, 20.0)?;
    assert_eq!(after_orbit, baked);
    assert_ne!(orbited.rgba, with.rgba);

    for mode in [2, 3, 4] {
        let (disabled, _) = frame(true, mode, 0.0)?;
        assert_eq!(scene_pixel(&disabled, midpoint)[0], lit, "mode {mode}");
    }
    let (spot_lit, before_spot) = frame(false, 5, 0.0)?;
    let (spot_shadow, after_spot) = frame(true, 5, 0.0)?;
    assert_eq!(after_spot, before_spot + 1);
    let lit = scene_pixel(&spot_lit, midpoint)[0];
    let occluded = scene_pixel(&spot_shadow, midpoint)[0];
    assert!(lit > 140 && occluded < 80, "spot: {lit} → {occluded}");

    let (mask_lit, _) = frame(false, 6, 0.0)?;
    let (mask_shadow, _) = frame(true, 6, 0.0)?;
    let left = [86, 100];
    let right = [113, 100];
    assert!(scene_pixel(&mask_lit, left)[0] > 140);
    assert!(
        scene_pixel(&mask_shadow, left)[0] < 80,
        "masked solid texels cast shadows"
    );
    assert_eq!(
        scene_pixel(&mask_lit, right),
        scene_pixel(&mask_shadow, right)
    );

    let (blend_unshadowed, before_blend) = frame(false, 7, 0.0)?;
    let (blend_shadowed, after_blend) = frame(true, 7, 0.0)?;
    assert_eq!(blend_unshadowed.rgba, blend_shadowed.rgba);
    assert_eq!(before_blend, after_blend);

    let (_, before_layers) = frame(false, 8, 0.0)?;
    let (both, after_layers) = frame(true, 8, 0.0)?;
    assert_eq!(after_layers, before_layers + 2);
    assert!(scene_pixel(&both, midpoint)[1] < 80);
    let (one, _) = frame(true, 11, 0.0)?;
    let center = scene_pixel(&one, midpoint);
    assert!(center[0] < 80 && center[1] > 140, "{center:?}");

    let (_, before_relight) = frame(true, 0, 0.0)?;
    let (_, relit) = frame(true, 9, 0.0)?;
    assert_eq!(relit, before_relight, "intensity does not invalidate depth");
    let (moved, after_move) = frame(true, 10, 0.0)?;
    assert_eq!(after_move, relit + 1);
    assert!(scene_pixel(&moved, midpoint)[0] > 140);
    let (unshadowed_point, point_before) = frame(false, 12, 0.0)?;
    let (shadowed_point, point_after) = frame(true, 12, 0.0)?;
    assert_eq!(
        point_after,
        point_before + 2,
        "only the two faces intersecting the caster"
    );
    // These rays hit opposite faces at the +X/-Z seam.
    for pixel in [[97, 100], [102, 100]] {
        let lit = scene_pixel(&unshadowed_point, pixel)[0];
        let shadow = scene_pixel(&shadowed_point, pixel)[0];
        assert!(
            lit > 140 && shadow < 80,
            "cube seam at {pixel:?}: {lit} → {shadow}"
        );
    }
    let (_, point_orbit) = frame(true, 12, 20.0)?;
    assert_eq!(point_orbit, point_after);
    let (_, point_relight) = frame(true, 15, 0.0)?;
    assert_eq!(point_relight, point_after);
    let (point_moved, point_refit) = frame(true, 13, 0.0)?;
    assert_eq!(
        point_refit,
        point_after + 1,
        "moved light sees the caster in one face"
    );
    assert_ne!(point_moved.rgba, shadowed_point.rgba);

    let (short_range, short_passes) = frame(true, 14, 0.0)?;
    let (short_baseline, short_baseline_passes) = frame(false, 14, 0.0)?;
    assert_eq!(short_passes, point_refit);
    assert_eq!(short_passes, short_baseline_passes);
    assert_eq!(short_range.rgba, short_baseline.rgba);
    let (disabled, _) = frame(true, 16, 0.0)?;
    assert_eq!(disabled.rgba, unshadowed_point.rgba);
    Ok(())
}

#[test]
fn per_light_shadow_sampling_reuses_depth_and_filters_across_point_faces() -> TestResult {
    use shadow_filters_ui::ShadowFiltersDemo;

    const SOURCE: &str = "examples/puzzle3d/shadow_filters.xr";
    const STRIDE: usize = 512 * 4;
    const VIEW_END: usize = STRIDE * 304;
    const CENTER: usize = 152 * STRIDE + 256 * 4;
    let module = xrack_compiler::prepare_preview(
        &std::fs::read_to_string(SOURCE)?,
        SOURCE,
        Default::default(),
    )?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&module.module, SOURCE, None)?;
    let dynamic = program.create("ShadowFiltersDemo")?;
    let app = ShadowFiltersDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let mut renderer = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut app = weak.upgrade().ok_or("missing shadow filters fixture")?;
    let mut draw = || -> TestResult<(Vec<u8>, u64)> {
        let image = aot.render(&mut renderer)?.to_vec();
        assert!(
            image == interpreter.render(&mut renderer)?,
            "backend shadow pixels"
        );
        let frame = xrack_assets::decode_png(&image)?;
        Ok((
            frame.rgba[..VIEW_END].to_vec(),
            renderer.scene_shadow_passes(),
        ))
    };

    for kind in ["directional", "spot", "point"] {
        app.set_light_kind(kind.into());
        dynamic.set("light-kind", Value::String(kind.into()))?;
        app.set_enabled(false);
        dynamic.set("enabled", Value::Bool(false))?;
        let (lit, _) = draw()?;
        app.set_enabled(true);
        dynamic.set("enabled", Value::Bool(true))?;
        app.set_filter_mode("hard".into());
        dynamic.set("filter-mode", Value::String("hard".into()))?;
        let (hard, passes) = draw()?;
        assert!(hard[CENTER].saturating_add(50) < lit[CENTER], "{kind}");

        app.set_strength(0.5);
        dynamic.set("strength", Value::Float(0.5))?;
        let (weaker, reused) = draw()?;
        assert!(
            hard[CENTER].saturating_add(25) < weaker[CENTER]
                && weaker[CENTER].saturating_add(15) < lit[CENTER],
            "{kind} center shadow strength"
        );
        assert_eq!(reused, passes);
        app.set_strength(0.0);
        dynamic.set("strength", Value::Float(0.0))?;
        let (cleared, reused) = draw()?;
        assert!(cleared == lit, "{kind} with zero shadow strength");
        assert_eq!(reused, passes);
        app.set_strength(1.0);
        dynamic.set("strength", Value::Float(1.0))?;

        if kind == "spot" {
            app.set_bias(0.05);
            dynamic.set("bias", Value::Float(0.05))?;
            let (biased, reused) = draw()?;
            assert!(biased != hard, "the spotlight uses its own receiver bias");
            assert_eq!(reused, passes);
            app.set_bias(0.001);
            dynamic.set("bias", Value::Float(0.001))?;
        }

        app.set_filter_mode("bilinear".into());
        dynamic.set("filter-mode", Value::String("bilinear".into()))?;
        let (bilinear, reused) = draw()?;
        assert_eq!(reused, passes);

        for mode in ["pcf4", "pcf8", "pcf16"] {
            app.set_filter_mode(mode.into());
            dynamic.set("filter-mode", Value::String(mode.into()))?;
            let (soft, reused) = draw()?;
            assert_eq!(reused, passes, "{kind}, {mode}");
            let filtered = hard
                .iter()
                .step_by(4)
                .zip(soft.iter().step_by(4))
                .zip(lit.iter().step_by(4))
                .filter(|&((&dark, &soft), &bright)| {
                    dark.saturating_add(12) < soft && soft.saturating_add(12) < bright
                })
                .count();
            assert!(filtered > 1000, "{kind}, {mode}, {filtered} pixels");
            if kind == "point" && mode == "pcf16" {
                // The two rays lie on opposite sides of the +X/-Z cube seam.
                let [a, b] = [soft[CENTER - 4], soft[CENTER]];
                assert!(a.abs_diff(b) < 8 && a > 70 && b > 70, "{a}, {b}");
            }
        }
        app.set_radius(0.0);
        dynamic.set("radius", Value::Float(0.0))?;
        let (zero_radius, reused) = draw()?;
        assert!(zero_radius == bilinear);
        assert_eq!(reused, passes);
        app.set_radius(10.0);
        dynamic.set("radius", Value::Float(10.0))?;
    }

    app.set_light_kind("directional".into());
    dynamic.set("light-kind", Value::String("directional".into()))?;
    app.set_cascades(3);
    dynamic.set("cascades", Value::Int(3))?;
    app.set_fill_light(true);
    dynamic.set("fill-light", Value::Bool(true))?;
    let (both, passes) = draw()?;
    app.set_strength(0.0);
    dynamic.set("strength", Value::Float(0.0))?;
    let (key_unshadowed, reused) = draw()?;
    assert_eq!(reused, passes);
    let mut changed = 0;
    for (before, after) in both
        .as_chunks::<4>()
        .0
        .iter()
        .zip(key_unshadowed.as_chunks::<4>().0)
    {
        changed += usize::from(after[0] > before[0].saturating_add(8));
        assert_eq!(before[1], after[1], "the green fill light stays shadowed");
    }
    assert!(
        changed > 20,
        "the red key light changes in cascaded shadows"
    );
    Ok(())
}

fn exercise_point_shadow_faces(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::PointShadows;

    let generated = PointShadows::new();
    let weak = generated.as_weak();
    let interpreted = fixture.create("PointShadows")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    for (face, axis) in ["+X", "-X", "+Y", "-Y", "+Z", "-Z"].into_iter().enumerate() {
        let mut generated = weak.upgrade().ok_or("missing point shadow scene")?;
        generated.set_face(face as i64);
        interpreted.set("face", Value::Int(face as i64))?;
        let passes = renderer.scene_shadow_passes();
        let mut pixels = [0; 2];
        for (index, enabled) in [false, true].into_iter().enumerate() {
            generated.set_enabled(enabled);
            interpreted.set("enabled", Value::Bool(enabled))?;
            let image = aot.render(renderer)?.to_vec();
            assert_eq!(image, dynamic.render(renderer)?, "point shadow {axis}");
            let image = xrack_assets::decode_png(&image)?;
            pixels[index] = scene_pixel(&image, [100, 100])[0];
            assert_eq!(
                renderer.scene_shadow_passes(),
                passes + index as u64,
                "{axis}"
            );
        }
        assert!(
            pixels[0] > 140 && pixels[1] < 80,
            "point shadow {axis}: {pixels:?}"
        );
    }
    Ok(())
}

fn exercise_directional_cascades(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::CascadeScene;

    let generated = CascadeScene::new();
    let weak = generated.as_weak();
    let interpreted = fixture.create("CascadeScene")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut dynamic = PreviewScene::new(Box::new(interpreted.clone()));
    let mut previous = None;
    let mut frame =
        |enable, distance, orbit, punctual| -> TestResult<(xrack_assets::Raster, u64)> {
            let mut generated = weak.upgrade().ok_or("missing cascade scene")?;
            generated.set_enabled(enable);
            generated.set_coverage(distance);
            generated.set_orbit(orbit);
            generated.set_punctual_on(punctual);
            interpreted.set("enabled", Value::Bool(enable))?;
            interpreted.set("coverage", Value::Float(distance))?;
            interpreted.set("orbit", Value::Float(orbit))?;
            interpreted.set("punctual-on", Value::Bool(punctual))?;
            let image = aot.render(renderer)?.to_vec();
            assert!(
                image == dynamic.render(renderer)?,
                "cascade AOT/interpret mismatch at yaw {orbit}"
            );
            let config = (enable, distance, punctual);
            let aot_node = aot.inspect_frame().ok_or("missing AOT frame")?.0;
            let dynamic_node = dynamic
                .inspect_frame()
                .ok_or("missing interpreter frame")?
                .0;
            if let Some((last, last_aot, last_dynamic)) = previous.as_ref()
                && *last == config
            {
                assert!(aot_node.same_frame_inputs(last_aot));
                assert!(dynamic_node.same_frame_inputs(last_dynamic));
            }
            previous = Some((config, aot_node.clone(), dynamic_node.clone()));
            Ok((
                xrack_assets::decode_png(&image)?,
                renderer.scene_shadow_passes(),
            ))
        };

    let near = [60, 100];
    let far = [140, 100];
    let (lit, before) = frame(false, 30.0, 0.0, false)?;
    let (shadows, baked) = frame(true, 30.0, 0.0, false)?;
    assert!((2..=3).contains(&(baked - before)), "populated cascades");
    for pixel in [near, far] {
        let (bright, dark) = (scene_pixel(&lit, pixel)[0], scene_pixel(&shadows, pixel)[0]);
        assert!(
            bright > 140 && dark < 80,
            "cascade at {pixel:?}: {bright} → {dark}"
        );
    }

    let (_, stable) = frame(true, 30.0, 0.0001, false)?;
    assert_eq!(
        stable, baked,
        "a sub-texel orbit should preserve the shadow maps"
    );
    let (_, moved) = frame(true, 30.0, 8.0, false)?;
    assert!((1..=3).contains(&(moved - stable)));
    let (_, mixed) = frame(true, 30.0, 0.0, true)?;
    assert!(
        mixed - moved >= baked - before + 2,
        "the point and spot maps must exist"
    );
    let (_, mixed_moved) = frame(true, 30.0, 8.0, true)?;
    assert_eq!(
        mixed_moved - mixed,
        moved - stable,
        "camera must leave the point and spot maps resident"
    );

    let (short, _) = frame(true, 12.0, 0.0, false)?;
    assert!(scene_pixel(&short, near)[0] < 80);
    assert_eq!(scene_pixel(&short, far), scene_pixel(&lit, far));
    let (blend, _) = frame(true, 17.65, 0.0, false)?;
    assert!(
        scene_pixel(&blend, near)[0] < 80,
        "the shadow must survive the split overlap"
    );
    Ok(())
}

fn exercise_environment(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::EnvironmentScene;

    #[derive(Clone, Copy)]
    struct Frame {
        enabled: bool,
        sky: bool,
        geometry: bool,
        yaw: f64,
        camera: f64,
        exposure: f64,
        intensity: f64,
    }

    let settings = Frame {
        enabled: true,
        sky: true,
        geometry: true,
        yaw: 0.0,
        camera: 0.0,
        exposure: 1.0,
        intensity: 1.0,
    };
    let app = EnvironmentScene::new();
    let weak = app.as_weak();
    let dynamic = fixture.create("EnvironmentScene")?;
    let mut generated = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut frame = |state: Frame| -> TestResult<(xrack_assets::Raster, u64)> {
        let mut app = weak
            .upgrade()
            .ok_or("missing generated environment scene")?;
        app.set_enabled(state.enabled);
        app.set_sky(state.sky);
        app.set_geometry(state.geometry);
        app.set_camera(state.camera);
        app.set_yaw(state.yaw);
        app.set_exposure(state.exposure);
        app.set_intensity(state.intensity);
        for (name, value) in [
            ("enabled", Value::Bool(state.enabled)),
            ("sky", Value::Bool(state.sky)),
            ("geometry", Value::Bool(state.geometry)),
            ("camera", Value::Float(state.camera)),
            ("yaw", Value::Float(state.yaw)),
            ("exposure", Value::Float(state.exposure)),
            ("intensity", Value::Float(state.intensity)),
        ] {
            dynamic.set(name, value)?;
        }
        let image = generated.render(renderer)?.to_vec();
        assert!(
            image == interpreted.render(renderer)?,
            "environment AOT/interpreter pixels disagree"
        );
        Ok((
            xrack_assets::decode_png(&image)?,
            renderer.scene_environment_uploads(),
        ))
    };

    let sharp = [62, 73];
    let rough = [138, 73];
    let diffuse = [100, 133];
    let sky = [100, 8];
    let (off, initial_uploads) = frame(Frame {
        enabled: false,
        ..settings
    })?;
    assert_eq!(scene_pixel(&off, diffuse), [0, 0, 0, 255]);
    let (lit, uploads) = frame(settings)?;
    // The interpreted puzzle and fixture use the same source probe.
    // Only the independently embedded AOT probe needs a new upload here.
    assert_eq!(uploads, initial_uploads + 1);
    let warm = scene_pixel(&lit, sharp);
    let broad = scene_pixel(&lit, rough);
    assert!(
        warm[0] > 155 && u16::from(warm[0]) > u16::from(warm[2]) + 50,
        "smooth reflected key: {warm:?}"
    );
    assert!(
        u16::from(warm[0]) > u16::from(broad[0]) + 25,
        "prefiltered roughness: {warm:?} > {broad:?}"
    );
    assert!(scene_pixel(&lit, diffuse)[0] > 50, "diffuse irradiance");
    let sky_pixel = scene_pixel(&lit, sky);
    assert!(
        u16::from(sky_pixel[2]) > u16::from(sky_pixel[0]) + 50,
        "sky at -Z: {sky_pixel:?}"
    );

    let (rotated, after_rotation) = frame(Frame {
        yaw: 180.0,
        ..settings
    })?;
    assert_eq!(after_rotation, uploads);
    let cool = scene_pixel(&rotated, sharp);
    assert!(
        u16::from(cool[2]) > u16::from(cool[0]) + 45,
        "rotated reflection: {cool:?}"
    );
    let (lower_exposure, after_exposure) = frame(Frame {
        exposure: 0.3,
        ..settings
    })?;
    assert_eq!(after_exposure, uploads);
    assert!(u16::from(scene_pixel(&lower_exposure, sky)[2]) + 15 < u16::from(sky_pixel[2]));
    let (no_sky, after_sky) = frame(Frame {
        sky: false,
        ..settings
    })?;
    assert_eq!(scene_pixel(&no_sky, sharp), warm);
    assert_eq!(after_sky, uploads);
    assert!(scene_pixel(&no_sky, sky)[2] < 20);
    let (sky_only, after_model) = frame(Frame {
        geometry: false,
        ..settings
    })?;
    assert_eq!(scene_pixel(&sky_only, sky), sky_pixel);
    assert_eq!(after_model, uploads);
    let (_, after_orbit) = frame(Frame {
        camera: 10.0,
        ..settings
    })?;
    assert_eq!(after_orbit, uploads);
    let (zero_probe, after_fade) = frame(Frame {
        intensity: 0.0,
        ..settings
    })?;
    assert_eq!(scene_pixel(&zero_probe, diffuse), [0, 0, 0, 255]);
    assert_eq!(after_fade, uploads);
    Ok(())
}

fn exercise_hierarchy(renderer: &mut Renderer, fixture: &xrack_interpreter::Program) -> TestResult {
    use scene3d_model_ui::HierarchyScene;
    use xrack_runtime::scene3d::{Ray3D, raycast};

    let app = HierarchyScene::new();
    let weak = app.as_weak();
    let instance = fixture.create("HierarchyScene")?;
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(instance.clone()));
    let mut previous_nodes = None;
    let mut frame =
        |mode, shift, orbit, expected: &str| -> TestResult<(xrack_assets::Raster, u64, u64)> {
            let mut app = weak.upgrade().ok_or("missing hierarchy scene")?;
            app.set_mode(mode);
            app.set_shift(shift);
            app.set_orbit(orbit);
            instance.set("mode", Value::Int(mode))?;
            instance.set("shift", Value::Float(shift))?;
            instance.set("orbit", Value::Float(orbit))?;
            assert_eq!(app.get_hit(), expected);
            assert_eq!(instance.get("hit")?, Value::String(expected.into()));
            let aot_pixels = aot.render(renderer)?.to_vec();
            assert!(
                aot_pixels == interpreted.render(renderer)?,
                "hierarchy AOT/interpreter differ"
            );
            let (aot_node, _, _) = aot.inspect_frame().ok_or("missing AOT hierarchy frame")?;
            let (runtime_node, layout, _) = interpreted
                .inspect_frame()
                .ok_or("missing interpreter hierarchy frame")?;
            if let Some((previous_aot, previous_runtime)) = &previous_nodes {
                assert!(
                    aot_node.same_frame_inputs(previous_aot),
                    "model updates must keep layout inputs"
                );
                assert!(runtime_node.same_frame_inputs(previous_runtime));
            }
            previous_nodes = Some((aot_node.clone(), runtime_node.clone()));
            let (view, _) = viewport(runtime_node, layout).ok_or("missing SceneView3D")?;
            let Some(Value::SceneObjects(objects)) = view.properties.get("model") else {
                return Err("missing prepared model".into());
            };
            assert_eq!(objects.len(), if mode == 0 { 2 } else { 1 });
            assert!(
                objects
                    .iter()
                    .all(|object| matches!(object.id.as_str(), "target" | "rear"))
            );
            if mode == 0 {
                let target = &objects[0];
                assert_eq!(target.id, "target");
                let rows = target.transform_rows();
                assert!(
                    (rows[0][3] - shift as f32).abs() < 0.000_01,
                    "parent-space pivot"
                );
                let ray =
                    Ray3D::new([shift as f32, 0.0, 8.0], [0.0, 0.0, -1.0]).ok_or("valid ray")?;
                let hit = raycast(objects, ray, 16.0).ok_or("missing transformed surface")?;
                assert_eq!(hit.object_id, "target");
                assert!((hit.distance - (8.0 - 0.5_f32.sqrt() * 0.5)).abs() < 0.0001);
                for (actual, expected) in hit.normal.into_iter().zip([0.4472136, 0.0, 0.8944272]) {
                    assert!(
                        (actual - expected).abs() < 0.0001,
                        "incorrect affine normal"
                    );
                }
            }
            Ok((
                xrack_assets::decode_png(&aot_pixels)?,
                renderer.scene_instance_uploads(),
                renderer.scene_shadow_passes(),
            ))
        };

    let (visible, uploaded, shadow_passes) = frame(0, 0.0, 0.0, "target")?;
    let center = [100, 100];
    let lit = scene_pixel(&visible, center);
    assert!(
        (138..=154).contains(&lit[0]),
        "inverse-transpose lighting: {lit:?}"
    );
    let (_, orbited, _) = frame(0, 0.0, 14.0, "target")?;
    assert_eq!(orbited, uploaded, "orbit must preserve the instance buffer");

    let (moved, after_move, moved_shadows) = frame(0, 1.6, 0.0, "rear")?;
    assert_eq!(
        after_move,
        uploaded + 1,
        "both backends reuse the changed instance set"
    );
    assert!(moved_shadows > shadow_passes);
    assert!(scene_pixel(&moved, center)[2] > 90);
    assert!(scene_pixel(&moved, [148, 100])[0] > 100);
    for mode in 1..=4 {
        let (hidden, _, passes) = frame(mode, 0.0, 0.0, "rear")?;
        assert!(scene_pixel(&hidden, center)[2] > 90);
        assert_eq!(
            passes, moved_shadows,
            "hidden or invalid branches cast no shadows"
        );
    }
    Ok(())
}

fn exercise_named_cameras(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::CameraScene;
    use xrack_runtime::{Length, Point3D, ProjectedPoint3D};

    let original = Point3D::new(0.65, 0.35, 0.0);
    let assert_world = |point: Point3D| {
        assert!((point.x - original.x).abs() < 2e-5, "{point:?}");
        assert!((point.y - original.y).abs() < 2e-5, "{point:?}");
        assert!((point.z - original.z).abs() < 2e-5, "{point:?}");
    };
    #[derive(Clone, Copy)]
    struct View<'a> {
        camera: &'a str,
        x: f64,
        yaw: f64,
        target_x: f64,
        target_visible: bool,
        visible: bool,
        near: f64,
        far: f64,
        fov: f64,
        width: f64,
        shadows: bool,
    }
    let defaults = View {
        camera: "perspective",
        x: 0.0,
        yaw: 0.0,
        target_x: 0.0,
        target_visible: true,
        visible: true,
        near: 0.1,
        far: 40.0,
        fov: 45.0,
        width: 200.0,
        shadows: false,
    };
    let generated = CameraScene::new();
    let weak = generated.as_weak();
    let dynamic = fixture.create("CameraScene")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut previous = None;
    let mut frame =
        |view: View<'_>, [center, edge]: [&str; 2]| -> TestResult<(xrack_assets::Raster, u64)> {
            let mut generated = weak.upgrade().ok_or("missing generated camera scene")?;
            generated.set_active(view.camera.into());
            generated.set_rig_x(view.x);
            generated.set_rig_yaw(view.yaw);
            generated.set_target_x(view.target_x);
            generated.set_target_visible(view.target_visible);
            generated.set_rig_visible(view.visible);
            generated.set_near(view.near);
            generated.set_far(view.far);
            generated.set_fov(view.fov);
            generated.set_view_width(Length(view.width));
            generated.set_shadows(view.shadows);
            for (name, value) in [
                ("active", Value::String(view.camera.into())),
                ("rig-x", Value::Float(view.x)),
                ("rig-yaw", Value::Float(view.yaw)),
                ("target-x", Value::Float(view.target_x)),
                ("target-visible", Value::Bool(view.target_visible)),
                ("rig-visible", Value::Bool(view.visible)),
                ("near", Value::Float(view.near)),
                ("far", Value::Float(view.far)),
                ("fov", Value::Float(view.fov)),
                ("view-width", Value::Length(Length(view.width))),
                ("shadows", Value::Bool(view.shadows)),
            ] {
                dynamic.set(name, value)?;
            }
            assert_eq!(generated.get_center_hit(), center);
            assert_eq!(generated.get_edge_hit(), edge);
            assert_eq!(dynamic.get("center-hit")?, Value::String(center.into()));
            assert_eq!(dynamic.get("edge-hit")?, Value::String(edge.into()));
            let hit = generated.get_camera_contact();
            assert_eq!(hit.object_id, center);
            assert_eq!(hit.valid, !center.is_empty());
            assert_eq!(dynamic.get("camera-contact")?, Value::Hit3D(hit));
            let ray = generated.get_ray_contact();
            assert!(ray.valid);
            assert_eq!(ray.object_id, "front");
            assert!((ray.distance - 7.9).abs() < 1e-5);
            assert!(ray.normal.z > 0.99);
            assert_eq!(dynamic.get("ray-contact")?, Value::Hit3D(ray));
            let projected = generated.get_projected();
            let restored = generated.get_restored();
            assert_eq!(
                dynamic.get("projected")?,
                Value::ProjectedPoint3D(projected)
            );
            assert_eq!(dynamic.get("restored")?, Value::Point3D(restored));
            assert_eq!(generated.get_screen_x(), projected.point.x);
            assert_eq!(dynamic.get("screen-x")?, Value::Length(projected.point.x));
            let orbit = generated.get_orbit_project();
            assert_eq!(
                dynamic.get("orbit-project")?,
                Value::ProjectedPoint3D(orbit)
            );
            assert_eq!(
                dynamic.get("orbit-unproject")?,
                Value::Point3D(generated.get_orbit_unproject())
            );
            assert_world(generated.get_orbit_unproject());

            if !view.visible || view.camera == "gone" || view.near >= view.far {
                assert_eq!(projected, ProjectedPoint3D::default());
                assert_eq!(restored, Point3D::new(-7.0, -11.0, -13.0));
            } else {
                assert!(projected.valid, "{projected:?}");
                assert_world(restored);
            }
            if view.near == 9.0 {
                assert!(
                    !projected.inside && projected.valid,
                    "behind near plane: {projected:?}"
                );
            }

            let image = aot.render(renderer)?.to_vec();
            assert!(
                image == interpreted.render(renderer)?,
                "camera AOT/interpreter mismatch"
            );
            let aot_node = aot.inspect_frame().ok_or("missing AOT camera frame")?.0;
            let interpreter_node = interpreted
                .inspect_frame()
                .ok_or("missing interpreter camera frame")?
                .0;
            if let Some((width, old_aot, old_interpreter)) = previous.as_ref()
                && *width == view.width
            {
                assert!(aot_node.same_frame_inputs(old_aot));
                assert!(interpreter_node.same_frame_inputs(old_interpreter));
            }
            previous = Some((view.width, aot_node.clone(), interpreter_node.clone()));
            Ok((
                xrack_assets::decode_png(&image)?,
                renderer.scene_instance_uploads(),
            ))
        };
    let (perspective, uploads) = frame(defaults, ["front", "front"])?;
    assert!(scene_pixel(&perspective, [124, 100])[0] > 180);
    let base = weak
        .upgrade()
        .ok_or("missing generated camera scene")?
        .get_projected();
    assert!(base.valid && base.inside);
    assert_eq!(base.depth, 8.0);
    let focal = 100.0 / (22.5_f64.to_radians().tan() * 8.0);
    assert!((base.point.x.0 - (100.0 + focal * 0.65)).abs() < 0.0001);
    assert!((base.point.y.0 - (100.0 - focal * 0.35)).abs() < 0.0001);
    assert!(
        scene_pixel(
            &perspective,
            [base.point.x.0 as usize, base.point.y.0 as usize]
        )[0] > 180
    );
    let (ortho, reused) = frame(
        View {
            camera: "map",
            ..defaults
        },
        ["front", ""],
    )?;
    assert_eq!(reused, uploads);
    assert_eq!(scene_pixel(&ortho, [124, 100]), [8, 9, 10, 255]);
    let (wide_fov, _) = frame(
        View {
            fov: 60.0,
            ..defaults
        },
        ["front", ""],
    )?;
    assert_eq!(scene_pixel(&wide_fov, [124, 100]), [8, 9, 10, 255]);
    let (clipped_near, _) = frame(
        View {
            near: 9.0,
            ..defaults
        },
        ["rear", ""],
    )?;
    assert!(scene_pixel(&clipped_near, [100, 100])[2] > 180);
    let (shadowed, _) = frame(
        View {
            camera: "map",
            near: 9.0,
            shadows: true,
            ..defaults
        },
        ["rear", ""],
    )?;
    assert!(
        (60..140).contains(&scene_pixel(&shadowed, [100, 100])[2]),
        "orthographic cascade must retain the camera-clipped caster"
    );
    let (clipped_both, _) = frame(
        View {
            near: 9.0,
            far: 11.0,
            ..defaults
        },
        ["", ""],
    )?;
    assert_eq!(scene_pixel(&clipped_both, [100, 100]), [8, 9, 10, 255]);

    let (moved, reuse_after_motion) = frame(View { x: 2.5, ..defaults }, ["side", ""])?;
    assert_eq!(reuse_after_motion, uploads);
    assert!(scene_pixel(&moved, [100, 100])[1] > 180);
    let (turned, _) = frame(
        View {
            yaw: 90.0,
            ..defaults
        },
        ["side", ""],
    )?;
    assert!(scene_pixel(&turned, [100, 100])[1] > 70);
    let (angled, _) = frame(
        View {
            camera: "angled",
            ..defaults
        },
        ["front", ""],
    )?;
    assert!(
        scene_pixel(&angled, [100, 100])[0] > 180,
        "camera basis must ignore inherited scaling"
    );
    let (wide, _) = frame(
        View {
            width: 300.0,
            ..defaults
        },
        ["front", ""],
    )?;
    assert_eq!(wide.size, [300, 200]);
    assert_eq!(scene_pixel(&wide, [124, 100]), [8, 9, 10, 255]);

    for view in [
        View {
            visible: false,
            ..defaults
        },
        View {
            camera: "gone",
            ..defaults
        },
        View {
            near: 3.0,
            far: 2.0,
            ..defaults
        },
    ] {
        let (blank, after) = frame(view, ["", ""])?;
        assert_eq!(after, uploads);
        assert_eq!(scene_pixel(&blank, [100, 100]), [8, 9, 10, 255]);
    }
    let (tracked, after_tracking) = frame(
        View {
            camera: "follow",
            target_x: 2.5,
            ..defaults
        },
        ["side", ""],
    )?;
    assert_eq!(after_tracking, uploads);
    assert!(scene_pixel(&tracked, [100, 100])[1] > 180);
    let (missing, _) = frame(
        View {
            camera: "follow",
            target_x: 2.5,
            target_visible: false,
            ..defaults
        },
        ["front", "front"],
    )?;
    assert_eq!(missing.rgba, perspective.rgba);
    let (pole, _) = frame(
        View {
            camera: "pole",
            ..defaults
        },
        ["front", ""],
    )?;
    assert!(
        scene_pixel(&pole, [100, 100])[0] > 60,
        "vertical look-at must have a valid basis"
    );
    Ok(())
}

fn exercise_mounted_lights(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::MountedLightScene;
    use xrack_runtime::scene3d::SceneLightSource;

    #[derive(Clone, Copy)]
    struct Frame<'a> {
        mode: i64,
        x: f64,
        yaw: f64,
        parent: &'a str,
        root_visible: bool,
        visible: bool,
        shadows: bool,
    }
    let defaults = Frame {
        mode: 0,
        x: 0.0,
        yaw: 0.0,
        parent: "joint",
        root_visible: true,
        visible: true,
        shadows: false,
    };
    let generated = MountedLightScene::new();
    let weak = generated.as_weak();
    let dynamic = fixture.create("MountedLightScene")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut previous = None;
    let mut frame =
        |state: Frame<'_>| -> TestResult<(xrack_assets::Raster, Vec<SceneLightSource>, u64, u64)> {
            let mut generated = weak.upgrade().ok_or("missing mounted light scene")?;
            generated.set_mode(state.mode);
            generated.set_mount_x(state.x);
            generated.set_mount_yaw(state.yaw);
            generated.set_parent_id(state.parent.into());
            generated.set_root_visible(state.root_visible);
            generated.set_light_visible(state.visible);
            generated.set_shadows(state.shadows);
            for (name, value) in [
                ("mode", Value::Int(state.mode)),
                ("mount-x", Value::Float(state.x)),
                ("mount-yaw", Value::Float(state.yaw)),
                ("parent-id", Value::String(state.parent.into())),
                ("root-visible", Value::Bool(state.root_visible)),
                ("light-visible", Value::Bool(state.visible)),
                ("shadows", Value::Bool(state.shadows)),
            ] {
                dynamic.set(name, value)?;
            }
            let image = aot.render(renderer)?.to_vec();
            assert!(
                image == interpreted.render(renderer)?,
                "mounted light AOT/interpreter pixels differ"
            );
            let node = aot.inspect_frame().ok_or("missing mounted viewport")?.0;
            if let Some(old) = &previous {
                assert!(node.same_frame_inputs(old));
            }
            previous = Some(node.clone());
            let (node, _) = inspect(&interpreted)?;
            let (Some(Value::SceneObjects(model)), Some(Value::SceneLights(lights))) =
                (node.properties.get("model"), node.properties.get("lights"))
            else {
                return Err("missing prepared scene".into());
            };
            let sources = lights
                .resolve(Some(model))
                .iter()
                .map(|light| light.source)
                .collect();
            Ok((
                xrack_assets::decode_png(&image)?,
                sources,
                renderer.scene_shadow_passes(),
                renderer.scene_instance_uploads(),
            ))
        };

    let center = [100, 100];
    let left = [70, 100];
    let right = [130, 100];
    let (directional, _, _, initial_uploads) = frame(defaults)?;
    assert!(scene_pixel(&directional, center)[0] > 180);
    let (turned, lights, _, _) = frame(Frame {
        yaw: 45.0,
        ..defaults
    })?;
    assert!(scene_pixel(&turned, center)[0] > 160);
    let [SceneLightSource::Directional { toward_light }] = lights.as_slice() else {
        return Err("missing mounted direction".into());
    };
    assert!((toward_light[0] - 0.5_f32.sqrt()).abs() < 0.0001);
    assert!((toward_light[2] - 0.5_f32.sqrt()).abs() < 0.0001);
    let (backward, _, _, _) = frame(Frame {
        yaw: 180.0,
        ..defaults
    })?;
    assert_eq!(scene_pixel(&backward, center)[0], 0);

    let (point, sources, _, _) = frame(Frame {
        mode: 1,
        ..defaults
    })?;
    assert!(scene_pixel(&point, left)[1] > scene_pixel(&point, right)[1].saturating_add(40));
    let [
        SceneLightSource::Point {
            position,
            inverse_range_squared,
        },
    ] = sources.as_slice()
    else {
        return Err("missing mounted point light".into());
    };
    assert_eq!(*position, [-1.0, 0.0, 2.0]);
    assert!((*inverse_range_squared - 1.0 / 9.0).abs() < 0.0001);
    let (moved_point, _, _, _) = frame(Frame {
        mode: 1,
        x: 2.0,
        ..defaults
    })?;
    assert!(
        scene_pixel(&moved_point, right)[1] > scene_pixel(&moved_point, left)[1].saturating_add(40)
    );

    let (spot, _, _, _) = frame(Frame {
        mode: 2,
        ..defaults
    })?;
    assert!(scene_pixel(&spot, right)[2] > 160);
    assert_eq!(scene_pixel(&spot, left)[2], 0);
    let (moved_spot, _, _, _) = frame(Frame {
        mode: 2,
        x: -2.0,
        ..defaults
    })?;
    assert!(scene_pixel(&moved_spot, left)[2] > 160);
    assert_eq!(scene_pixel(&moved_spot, right)[2], 0);
    for config in [
        Frame {
            root_visible: false,
            ..defaults
        },
        Frame {
            parent: "unresolved",
            ..defaults
        },
        Frame {
            visible: false,
            ..defaults
        },
    ] {
        let (unlit, lights, _, _) = frame(config)?;
        assert!(lights.is_empty());
        assert_eq!(scene_pixel(&unlit, center), [0, 0, 0, 255]);
    }
    let (_, _, before, _) = frame(Frame {
        mode: 3,
        ..defaults
    })?;
    let (shadowed, _, baked, _) = frame(Frame {
        mode: 3,
        shadows: true,
        ..defaults
    })?;
    assert_eq!(baked, before + 2);
    let (moved, _, updated, uploads) = frame(Frame {
        mode: 3,
        shadows: true,
        x: -2.0,
        ..defaults
    })?;
    assert_eq!(
        updated,
        baked + 1,
        "the unchanged directional shadow stays resident"
    );
    assert_eq!(
        uploads, initial_uploads,
        "unrendered mount motion must not upload geometry"
    );
    assert_ne!(moved.rgba, shadowed.rgba);
    Ok(())
}

fn exercise_gltf_animation(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::AnimatedScene;
    struct Frame {
        image: xrack_assets::Raster,
        rows: [[f32; 4]; 3],
        mesh_uploads: u64,
        shadow_passes: u64,
    }

    let initial_uploads = renderer.scene_mesh_uploads();
    let initial_shadow_passes = renderer.scene_shadow_passes();
    let app = AnimatedScene::new();
    let weak = app.as_weak();
    let dynamic = fixture.create("AnimatedScene")?;
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut previous = None;
    let mut frame = |clip: &str, time, looping, orbit, hit: &str| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing animated scene")?;
        app.set_clip(clip.into());
        app.set_time(time);
        app.set_loop(looping);
        app.set_orbit(orbit);
        dynamic.set("clip", Value::String(clip.into()))?;
        dynamic.set("time", Value::Float(time))?;
        dynamic.set("loop", Value::Bool(looping))?;
        dynamic.set("orbit", Value::Float(orbit))?;
        assert_eq!(app.get_hit(), hit, "AOT {clip} at {time}");
        assert_eq!(
            dynamic.get("hit")?,
            Value::String(hit.into()),
            "{clip} at {time}"
        );
        let image = aot.render(renderer)?.to_vec();
        assert!(
            image == interpreted.render(renderer)?,
            "{clip} at {time}: AOT/interpreter pose differs"
        );
        let root = interpreted.inspect_frame().ok_or("missing scene root")?.0;
        if let Some(old) = &previous {
            assert!(
                root.same_frame_inputs(old),
                "clip and pose updates preserve layout"
            );
        }
        previous = Some(root.clone());
        let (viewport, _) = inspect(&interpreted)?;
        let Some(Value::SceneObjects(model)) = viewport.properties.get("model") else {
            return Err("missing animated model".into());
        };
        assert_eq!(model.len(), 3);
        assert_eq!(model[0].id, "mover");
        assert_eq!(model[1].id, "mover");
        Ok(Frame {
            image: xrack_assets::decode_png(&image)?,
            rows: model[0].transform_rows(),
            mesh_uploads: renderer.scene_mesh_uploads(),
            shadow_passes: renderer.scene_shadow_passes(),
        })
    };

    let rest = frame("", 0.0, false, 0.0, "mover")?;
    assert_eq!(
        rest.mesh_uploads,
        initial_uploads + 2,
        "two glTF nodes share one mesh per backend"
    );
    assert!(rest.shadow_passes > initial_shadow_passes);
    let midpoint = frame("Slide", 0.5, false, 0.0, "wall")?;
    assert_eq!(midpoint.rows[0][3], 1.0);
    let end = frame("Slide", 4.0, false, 0.0, "wall")?;
    assert_eq!(end.rows[0][3], 2.0);
    for animated in [&midpoint, &end] {
        assert_eq!(
            animated.mesh_uploads, rest.mesh_uploads,
            "poses reuse resident vertex data"
        );
    }
    assert!(end.shadow_passes > rest.shadow_passes);
    let shadow = [127, 100];
    let blocked = scene_pixel(&rest.image, shadow)[2];
    let lit = scene_pixel(&end.image, shadow)[2];
    assert!(
        lit > blocked.saturating_add(55),
        "animated occluder: {blocked} → {lit}"
    );

    let orbit = frame("Slide", 0.5, false, 15.0, "wall")?;
    assert_eq!(orbit.mesh_uploads, rest.mesh_uploads);
    let looped = frame("Slide", 1.5, true, 0.0, "wall")?;
    assert_eq!(looped.image.rgba, midpoint.image.rgba);
    assert_eq!(
        frame("#0", 0.5, false, 0.0, "wall")?.image.rgba,
        midpoint.image.rgba
    );
    assert_eq!(
        frame("missing", 3.0, false, 0.0, "mover")?.image.rgba,
        rest.image.rgba
    );

    let stepped = frame("Step", 0.75, false, 0.0, "mover")?;
    assert_eq!(stepped.rows[0][3], 0.0);
    assert_eq!(stepped.image.rgba, rest.image.rgba);
    let curve = frame("Curve", 1.0, false, 0.0, "mover")?;
    assert!(
        (curve.rows[0][0] - 2.0).abs() < 0.0001,
        "Hermite tangents use seconds"
    );
    let turned = frame("Turn", 0.5, false, 0.0, "mover")?;
    assert!((turned.rows[0][0] - 0.5_f32.sqrt()).abs() < 0.0001);
    assert!(
        (turned.rows[1][0] - 0.5_f32.sqrt()).abs() < 0.0001,
        "shortest quaternion path"
    );
    assert_eq!(turned.mesh_uploads, rest.mesh_uploads);

    let mirrored = frame("Mirror", 0.0, false, 0.0, "mover")?;
    assert!(
        scene_pixel(&mirrored.image, [100, 100])[0] > 220,
        "mirrored single-sided mesh stays visible"
    );
    assert_eq!(
        mirrored.mesh_uploads, rest.mesh_uploads,
        "negative scale animation shares resident mesh buffers"
    );
    assert_eq!(
        frame("Mirror", 0.5, false, 0.0, "mover")?.mesh_uploads,
        mirrored.mesh_uploads
    );
    Ok(())
}

fn exercise_skinning(renderer: &mut Renderer, fixture: &xrack_interpreter::Program) -> TestResult {
    use scene3d_model_ui::SkinnedScene;
    struct Frame {
        image: xrack_assets::Raster,
        mesh_uploads: u64,
        palette_uploads: u64,
        instance_uploads: u64,
        shadow_passes: u64,
    }
    let initial_meshes = renderer.scene_mesh_uploads();
    let initial_palettes = renderer.scene_skin_uploads();
    let app = SkinnedScene::new();
    let weak = app.as_weak();
    let dynamic = fixture.create("SkinnedScene")?;
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut frame = |time, orbit, center: &str, side: &str| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing skinned scene")?;
        app.set_time(time);
        app.set_orbit(orbit);
        dynamic.set("time", Value::Float(time))?;
        dynamic.set("orbit", Value::Float(orbit))?;
        assert_eq!(app.get_center_hit(), center, "AOT skinned center at {time}");
        assert_eq!(app.get_side_hit(), side, "AOT skinned side at {time}");
        assert_eq!(dynamic.get("center-hit")?, Value::String(center.into()));
        assert_eq!(dynamic.get("side-hit")?, Value::String(side.into()));
        let image = aot.render(renderer)?.to_vec();
        assert!(
            image == interpreted.render(renderer)?,
            "skin AOT and interpreter pixels differ"
        );
        let (viewport, _) = inspect(&interpreted)?;
        let Some(Value::SceneObjects(objects)) = viewport.properties.get("model") else {
            return Err("missing posed skin".into());
        };
        assert_eq!(objects.len(), 3);
        let palette = objects[0].skin_palette().ok_or("missing joint palette")?;
        assert_eq!(palette.len(), 2);
        assert_eq!(
            objects[0].transform_rows()[0][3],
            0.0,
            "glTF skin mesh node offset must not apply"
        );
        assert!((palette[1][0][3] - (time.min(1.0) * 2.0) as f32).abs() < 0.0001);
        assert!(
            objects[1].skin_palette().is_none(),
            "rigid attachment stays a rigid draw"
        );
        assert!(
            !objects[0].mesh.tangents().is_empty(),
            "exercise the skinned normal map path"
        );
        Ok(Frame {
            image: xrack_assets::decode_png(&image)?,
            mesh_uploads: renderer.scene_mesh_uploads(),
            palette_uploads: renderer.scene_skin_uploads(),
            instance_uploads: renderer.scene_instance_uploads(),
            shadow_passes: renderer.scene_shadow_passes(),
        })
    };

    let rest = frame(0.0, 0.0, "skin", "wall")?;
    assert_eq!(rest.mesh_uploads, initial_meshes + 4);
    assert_eq!(
        rest.palette_uploads,
        initial_palettes + 1,
        "shared palette for both passes and backends"
    );
    assert!(scene_pixel(&rest.image, [100, 100])[0] > 100);
    let posed = frame(1.0, 0.0, "wall", "skin")?;
    assert_eq!(posed.mesh_uploads, rest.mesh_uploads);
    assert_eq!(posed.instance_uploads, rest.instance_uploads);
    assert_eq!(posed.palette_uploads, rest.palette_uploads + 1);
    assert!(posed.shadow_passes > rest.shadow_passes);
    assert!(scene_pixel(&posed.image, [130, 100])[0] > 100);
    let old_shadow = scene_pixel(&rest.image, [72, 100])[2];
    let new_light = scene_pixel(&posed.image, [72, 100])[2];
    assert!(
        new_light > old_shadow.saturating_add(55),
        "skinned shadow moved: {old_shadow} → {new_light}"
    );
    let orbited = frame(1.0, 12.0, "wall", "skin")?;
    assert_eq!(
        orbited.palette_uploads, posed.palette_uploads,
        "camera-only motion reuses skin uniforms"
    );
    assert_eq!(orbited.mesh_uploads, posed.mesh_uploads);
    Ok(())
}

fn exercise_morph_targets(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::MorphScene;
    struct Frame {
        image: xrack_assets::Raster,
        hits: [String; 3],
        weights: [Vec<f32>; 2],
        mesh_uploads: u64,
        palette_uploads: u64,
        morph_uploads: u64,
        instance_uploads: u64,
        shadow_passes: u64,
    }

    let initial_meshes = renderer.scene_mesh_uploads();
    let initial_morphs = renderer.scene_morph_uploads();
    let app = MorphScene::new();
    let weak = app.as_weak();
    let dynamic = fixture.create("MorphScene")?;
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut frame = |clip: &str, time, orbit| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing morph scene")?;
        app.set_clip(clip.into());
        app.set_time(time);
        app.set_orbit(orbit);
        dynamic.set("clip", Value::String(clip.into()))?;
        dynamic.set("time", Value::Float(time))?;
        dynamic.set("orbit", Value::Float(orbit))?;
        let hits = [
            app.get_center_hit(),
            app.get_side_hit(),
            app.get_upper_hit(),
        ]
        .map(|hit| hit.to_string());
        for (property, expected) in ["center-hit", "side-hit", "upper-hit"]
            .into_iter()
            .zip(&hits)
        {
            assert_eq!(
                dynamic.get(property)?,
                Value::String(expected.as_str().into())
            );
        }
        let image = aot.render(renderer)?.to_vec();
        assert_eq!(
            image.as_slice(),
            interpreted.render(renderer)?,
            "morph AOT/interpreter raster mismatch for {clip} at {time}"
        );
        let (viewport, _) = inspect(&interpreted)?;
        let Some(Value::SceneObjects(objects)) = viewport.properties.get("model") else {
            return Err("missing morph model".into());
        };
        assert_eq!(objects.len(), 3);
        assert!(objects[0].skin_palette().is_some());
        assert!(objects[1].skin_palette().is_none());
        let weights = std::array::from_fn(|index| {
            let object = &objects[index];
            assert!(object.mesh.morph_geometry().is_some());
            assert!(!object.mesh.tangents().is_empty());
            object
                .morph_weights()
                .map_or_else(Vec::new, |weights| weights.to_vec())
        });
        Ok(Frame {
            image: xrack_assets::decode_png(&image)?,
            hits,
            weights,
            mesh_uploads: renderer.scene_mesh_uploads(),
            palette_uploads: renderer.scene_skin_uploads(),
            morph_uploads: renderer.scene_morph_uploads(),
            instance_uploads: renderer.scene_instance_uploads(),
            shadow_passes: renderer.scene_shadow_passes(),
        })
    };
    let rest = frame("", 0.0, 0.0)?;
    assert_eq!(rest.weights, [vec![0.25, 0.0], vec![]]);
    assert_eq!(rest.hits, ["panel", "wall", "wall"]);
    assert_eq!(rest.mesh_uploads, initial_meshes + 4);
    assert_eq!(rest.morph_uploads, initial_morphs + 1);
    assert!(scene_pixel(&rest.image, [100, 100])[0] > 100);

    let posed = frame("Morph", 0.5, 0.0)?;
    assert_eq!(posed.weights, [vec![0.625, 0.25], vec![0.625, 0.5]]);
    assert_eq!(
        posed.hits,
        ["wall", "panel", "panel"],
        "morph precedes skin"
    );
    assert_eq!(posed.mesh_uploads, rest.mesh_uploads);
    assert_eq!(posed.instance_uploads, rest.instance_uploads);
    assert_eq!(posed.palette_uploads, rest.palette_uploads);
    assert_eq!(posed.morph_uploads, rest.morph_uploads + 2);
    assert!(posed.shadow_passes > rest.shadow_passes);
    assert!(scene_pixel(&posed.image, [175, 100])[0] > 100);
    assert!(scene_pixel(&posed.image, [122, 58])[0] > 100);
    let blocked = scene_pixel(&rest.image, [79, 100])[2];
    let lit = scene_pixel(&posed.image, [79, 100])[2];
    assert!(
        lit > blocked.saturating_add(55),
        "morph shadow moved: {blocked} → {lit}"
    );

    let orbit = frame("Morph", 0.5, 12.0)?;
    assert_eq!(orbit.morph_uploads, posed.morph_uploads);
    assert_eq!(orbit.mesh_uploads, posed.mesh_uploads);
    let before_step = frame("StepMorph", 0.75, 0.0)?;
    assert_eq!(before_step.image.rgba, rest.image.rgba);
    let after_step = frame("StepMorph", 1.0, 0.0)?;
    assert_eq!(after_step.weights, [vec![0.25, 0.0], vec![1.0, 0.0]]);
    assert_eq!(after_step.mesh_uploads, rest.mesh_uploads);
    Ok(())
}

fn exercise_morph_overrides(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::MorphControlScene;

    let generated = MorphControlScene::new();
    let weak = generated.as_weak();
    let dynamic = fixture.create("MorphControlScene")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut uploads = None;
    let mut frame = |extension, time, weights: [[f32; 2]; 2], hits: [&str; 3]| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing scene")?;
        app.set_extension(extension);
        app.set_time(time);
        dynamic.set("extension", Value::Float(extension))?;
        dynamic.set("time", Value::Float(time))?;
        assert_eq!(
            [
                app.get_center_hit(),
                app.get_side_hit(),
                app.get_upper_hit()
            ],
            hits
        );
        for (name, expected) in ["center-hit", "side-hit", "upper-hit"]
            .into_iter()
            .zip(hits)
        {
            assert_eq!(dynamic.get(name)?, Value::String(expected.into()));
        }
        let contact = app.get_driven_contact();
        assert_eq!(
            dynamic.get("driven-contact")?,
            Value::Hit3D(contact.clone())
        );
        assert!(contact.valid);
        if extension == 0.75 {
            assert_eq!(contact.object_id, "panel");
            assert!((contact.position.z - 0.0).abs() < 1e-5);
            assert!((contact.local_position.x - 2.45).abs() < 1e-5);
            assert!(contact.normal.z > 0.99);
        } else {
            assert_eq!(contact.object_id, "wall");
        }
        assert_eq!(app.get_twin_hit(), "twin");
        assert_eq!(dynamic.get("twin-hit")?, Value::String("twin".into()));
        let pixels = aot.render(renderer)?.to_vec();
        assert_eq!(pixels.as_slice(), interpreted.render(renderer)?);
        let (view, _) = inspect(&interpreted)?;
        let Some(Value::SceneObjects(objects)) = view.properties.get("model") else {
            return Err("missing scene for morph override".into());
        };
        assert_eq!(objects.len(), 5);
        assert!(
            objects[0].mesh == objects[2].mesh,
            "instances share one mesh"
        );
        assert_eq!(
            objects[2].morph_weights().map(|weights| weights.as_ref()),
            Some(&[0.2, 0.0][..]),
            "the twin has its own weights",
        );
        for (object, expected) in objects.iter().zip(weights) {
            let target_weights = object.morph_weights().ok_or("missing driven weights")?;
            assert_eq!(target_weights.as_ref(), &expected);
        }
        let counts = [
            renderer.scene_mesh_uploads(),
            renderer.scene_instance_uploads(),
            renderer.scene_skin_uploads(),
        ];
        if let Some(previous) = uploads {
            assert_eq!(
                counts, previous,
                "reuse unchanged geometry and instance data"
            );
        }
        uploads = Some(counts);
        Ok((
            xrack_assets::decode_png(&pixels)?,
            renderer.scene_shadow_passes(),
        ))
    };
    let (low, shadows_at_low) = frame(
        0.0,
        0.5,
        [[0.0, 0.25], [0.125, 0.0]],
        ["panel", "wall", "wall"],
    )?;
    let (high, shadows_at_high) = frame(
        0.75,
        0.5,
        [[0.75, 0.25], [0.125, 0.75]],
        ["wall", "panel", "panel"],
    )?;
    assert_ne!(low.rgba, high.rgba);
    assert!(shadows_at_high > shadows_at_low);
    assert!(scene_pixel(&high, [175, 100])[0] > 100);
    assert!(scene_pixel(&high, [84, 58])[0] > 100);
    let twin = scene_pixel(&high, [55, 142]);
    assert!(twin[1] > twin[0] && twin[1] > twin[2]);
    assert_eq!(
        twin,
        scene_pixel(&low, [55, 142]),
        "twin's pose stays fixed"
    );
    frame(
        0.75,
        1.0,
        [[0.75, 0.5], [0.125, 0.75]],
        ["wall", "panel", "panel"],
    )?;
    frame(
        f64::INFINITY,
        0.5,
        [[0.125, 0.25], [0.125, 0.5]],
        ["panel", "wall", "panel"],
    )?;
    Ok(())
}

fn exercise_hit_lists(renderer: &mut Renderer, fixture: &xrack_interpreter::Program) -> TestResult {
    use scene3d_model_ui::HitListScene;
    use xrack_runtime::Hit3D;

    let mut generated = HitListScene::new();
    let dynamic = fixture.create("HitListScene")?;
    let dynamic_hits = |name| -> TestResult<Vec<Hit3D>> {
        let Value::Hit3DList(rows) = dynamic.get(name)? else {
            return Err("expected a typed hit list".into());
        };
        Ok(rows.into_snapshot())
    };
    let hits = generated.get_hits();
    assert_eq!(hits, dynamic_hits("hits")?);
    assert_eq!(hits.len(), 5);
    assert_eq!(
        hits.iter().map(|hit| hit.row_index).collect::<Vec<_>>(),
        [1, 5, 2, 3, 6],
        "original rows survive invalid, anonymous, and duplicate-ID objects",
    );
    assert_eq!(
        hits.iter()
            .map(|hit| hit.object_id.as_str())
            .collect::<Vec<_>>(),
        ["front", "", "rig", "rear", "front"],
    );
    assert!(hits.iter().all(|hit| hit.valid));
    assert_eq!(
        hits.iter().map(|hit| hit.distance).collect::<Vec<_>>(),
        [5.5, 6.5, 8.0, 9.5, 11.5],
        "only the nearest of the glTF row's two mesh placements is returned",
    );
    assert_eq!(generated.get_count(), 5);
    assert_eq!(generated.get_selected_row(), 1);
    assert_eq!(generated.get_selected(), hits[0]);
    assert_eq!(dynamic.get("selected-row")?, Value::Int(1));
    assert!(!generated.get_out_of_range().valid);
    let clipped = generated.get_clipped();
    assert_eq!(dynamic_hits("clipped")?, clipped);
    assert_eq!(clipped.len(), 4);
    assert_eq!(clipped[0].row_index, 5);
    assert_eq!(
        clipped[0].distance, 7.5,
        "the near plane clips this box's front face"
    );
    assert_eq!(clipped[1..], hits[2..]);
    assert_eq!(generated.get_from_world(), hits);
    assert_eq!(dynamic_hits("from-world")?, hits);
    assert!(generated.get_no_short_hits());
    assert!(generated.get_invalid_camera().is_empty());
    assert!(dynamic_hits("invalid-camera")?.is_empty());

    generated.invoke_collect();
    dynamic.invoke("collect", &[])?;
    assert_eq!(generated.get_visited(), "1 5 2 3 6 ");
    assert_eq!(dynamic.get("visited")?, Value::String("1 5 2 3 6 ".into()));
    assert_eq!(generated.get_edited(), [hits[1].clone()]);
    assert_eq!(dynamic_hits("edited")?, [hits[1].clone()]);
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    assert_eq!(
        aot.render(renderer)?.to_vec(),
        interpreted.render(renderer)?
    );
    fn labels(node: &Node, layout: &LayoutNode, rows: &mut Vec<String>) {
        if let Some(text) = node.text("text").filter(|text| text.starts_with("row ")) {
            rows.push(text.to_owned());
        }
        for (_, node, layout) in layout.children_in_paint_order(node) {
            labels(node, layout, rows);
        }
    }
    let (node, layout, _) = interpreted.inspect_frame().ok_or("missing repeated hits")?;
    let mut rows = Vec::new();
    labels(node, layout, &mut rows);
    assert_eq!(
        rows,
        [
            "row 1 front",
            "row 5 ",
            "row 2 rig",
            "row 3 rear",
            "row 6 front"
        ]
    );
    Ok(())
}

fn exercise_frustum_culling(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::{CameraRow, CullScene};
    use xrack_runtime::{Color, RecordList, RecordValue};

    let camera = |x: f64| CameraRow {
        id: "view".into(),
        mesh: String::new(),
        x,
        z: 8.0,
        scale_x: 1.0,
        scale_y: 1.0,
        scale_z: 1.0,
        visible: true,
        camera_kind: "orthographic".into(),
        camera_fov: 45.0,
        camera_height: 8.0,
        camera_near: 0.1,
        camera_far: 40.0,
        ..Default::default()
    };
    let cuboid = |x, y, z| CameraRow {
        mesh: "primitive:box".into(),
        visible: true,
        x,
        y,
        z,
        scale_x: 0.42,
        scale_y: 0.42,
        scale_z: 0.42,
        color: Color(0xff7020ff),
        ..Default::default()
    };
    let record = |row: &CameraRow| {
        RecordValue::new(
            "CameraRow",
            [
                ("id", Value::String(row.id.clone())),
                ("mesh", Value::String(row.mesh.clone())),
                ("parent", Value::String(row.parent.clone())),
                ("visible", Value::Bool(row.visible)),
                ("x", Value::Float(row.x)),
                ("y", Value::Float(row.y)),
                ("z", Value::Float(row.z)),
                ("scale-x", Value::Float(row.scale_x)),
                ("scale-y", Value::Float(row.scale_y)),
                ("scale-z", Value::Float(row.scale_z)),
                ("yaw", Value::Float(row.yaw)),
                ("pitch", Value::Float(row.pitch)),
                ("roll", Value::Float(row.roll)),
                ("color", Value::Color(row.color)),
                ("camera-kind", Value::String(row.camera_kind.clone())),
                ("camera-fov", Value::Float(row.camera_fov)),
                ("camera-height", Value::Float(row.camera_height)),
                ("camera-near", Value::Float(row.camera_near)),
                ("camera-far", Value::Float(row.camera_far)),
                ("camera-look-at", Value::String(row.camera_look_at.clone())),
            ]
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
        )
    };

    let mut scene = vec![camera(0.0)];
    for index in 0..512 {
        if index % 8 == 0 {
            scene.push(cuboid(
                (index / 8 % 8) as f64 * 0.75 - 2.625,
                (index / 64) as f64 * 0.75 - 2.625,
                0.0,
            ));
        } else {
            scene.push(cuboid(40.0 + (index % 16) as f64, 0.0, 0.0));
        }
    }
    scene.push(cuboid(4.05, 0.0, 0.0)); // intersects the camera's right plane
    scene.push(cuboid(0.0, 0.0, -50.0)); // beyond the far plane
    scene.push(cuboid(0.0, 0.0, 10.0)); // behind the camera
    let object_count = (scene.len() - 1) as u64;
    let records = RecordList::new("CameraRow", scene.iter().map(record).collect());
    let generated_rows = VecModel::from(scene);
    let mut generated = CullScene::new();
    generated.bind_model_actors(generated_rows.clone());
    let weak = generated.as_weak();
    let dynamic = fixture.create("CullScene")?;
    dynamic.set("actors", Value::RecordList(records.clone()))?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut frame = |cull, camera_x| -> TestResult<_> {
        weak.upgrade()
            .ok_or("missing AOT cull scene")?
            .set_cull(cull);
        dynamic.set("cull", Value::Bool(cull))?;
        generated_rows.set_row(0, camera(camera_x));
        records.model().set_row(0, record(&camera(camera_x)));
        let before = renderer.scene_draw_stats();
        let pixels = aot.render(renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert_eq!(pixels.as_slice(), interpreted.render(renderer)?);
        Ok((
            pixels,
            [
                after.color_draws - before.color_draws,
                after.color_instances - before.color_instances,
                after.culled_instances - before.culled_instances,
            ],
            after.color_compaction_uploads,
            renderer.scene_instance_uploads(),
            after,
        ))
    };

    let (baseline, draws, compactions, instances, _) = frame(false, 0.0)?;
    assert_eq!(draws, [1, object_count, 0]);
    let (culled, draws, packed, reused_instances, _) = frame(true, 0.0)?;
    assert_eq!(culled, baseline);
    assert_eq!(draws, [1, 65, object_count - 65]);
    assert_eq!(packed, compactions + 1);
    assert_eq!(reused_instances, instances);
    let (moved, draws, after_move, reused_instances, _) = frame(true, 0.05)?;
    assert_ne!(moved, culled);
    assert_eq!(draws, [1, 65, object_count - 65]);
    assert_eq!(
        after_move, packed,
        "movement that keeps the same visible mask reuses the compact buffer"
    );
    assert_eq!(reused_instances, instances);
    let (empty, draws, _, _, _) = frame(true, -80.0)?;
    assert_eq!(draws, [0, 0, object_count]);
    assert_eq!(
        scene_pixel(&xrack_assets::decode_png(&empty)?, [100, 100]),
        [8, 9, 10, 255]
    );
    // A fragmented batch still has one draw, but its compacted buffer
    // contains only visible rows. Check both buffers against single-row edits.
    let (original, _, _, _, before) = frame(true, 0.0)?;
    let render_node = || {
        weak.upgrade()
            .ok_or("missing AOT cull scene")
            .map(|app| app.render())
    };
    let before_edit = render_node()?;
    let tint = |index: usize, color| -> TestResult {
        let mut row = generated_rows.row(index).ok_or("missing edited cube")?;
        row.color = color;
        generated_rows.set_row(index, row.clone());
        records.model().set_row(index, record(&row));
        Ok(())
    };
    tint(1, Color(0x38df8cff))?;
    let (edited, _, _, _, tinted) = frame(true, 0.0)?;
    let after_edit = render_node()?;
    assert_ne!(edited, original);
    assert_eq!(
        tinted.instance_upload_bytes - before.instance_upload_bytes,
        2 * 68
    );
    assert_eq!(
        tinted.partial_instance_uploads - before.partial_instance_uploads,
        2
    );
    assert_eq!(
        tinted.instance_buffer_allocations,
        before.instance_buffer_allocations
    );

    tint(2, Color(0x9ab1ffff))?;
    let (hidden_edit, _, _, _, hidden) = frame(true, 0.0)?;
    assert_eq!(hidden_edit, edited);
    assert_eq!(
        hidden.instance_upload_bytes - tinted.instance_upload_bytes,
        68
    );
    assert_eq!(
        hidden.partial_instance_uploads - tinted.partial_instance_uploads,
        1
    );
    assert_eq!(
        hidden.instance_buffer_allocations,
        tinted.instance_buffer_allocations
    );

    for index in 1..=300 {
        tint(index, Color(0x5568fcff))?;
    }
    let (_, _, _, _, dense) = frame(true, 0.0)?;
    assert_eq!(
        dense.instance_buffer_allocations - hidden.instance_buffer_allocations,
        2
    );
    assert_eq!(
        dense.partial_instance_uploads,
        hidden.partial_instance_uploads
    );
    assert_eq!(
        dense.instance_upload_bytes - hidden.instance_upload_bytes,
        (object_count + 65) * 68
    );

    // wgpu queue writes run before the whole submission. Two views of
    // different model snapshots in that submission must keep separate data.
    let mut guarded = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let input = xrack_layout::Interaction::default();
    let old = guarded.snapshot(&before_edit, 200, 200, 1.0, &input)?;
    let changed = renderer.snapshot(&after_edit, 200, 200, 1.0, &input)?;
    assert_ne!(old.rgba, changed.rgba);
    let mut paired = Node::new("Window", None);
    let view_of = |root: &Node| {
        root.children
            .iter()
            .find(|node| node.kind == "SceneView3D")
            .cloned()
    };
    paired
        .children
        .push(view_of(&before_edit).ok_or("missing original viewport")?);
    let mut right = view_of(&after_edit).ok_or("missing edited viewport")?;
    right
        .properties
        .insert("x", Value::Length(xrack_runtime::Length(200.0)));
    paired.children.push(right);
    let before = guarded.scene_draw_stats();
    let combined = guarded.snapshot(&paired, 400, 200, 1.0, &input)?;
    let after = guarded.scene_draw_stats();
    assert_eq!(
        after.partial_instance_uploads,
        before.partial_instance_uploads
    );
    assert_eq!(
        after.instance_buffer_allocations - before.instance_buffer_allocations,
        2
    );
    for (y, row) in combined.rgba.chunks_exact(400 * 4).enumerate() {
        let source = y * 200 * 4..(y + 1) * 200 * 4;
        assert_eq!(&row[..200 * 4], &old.rgba[source.clone()]);
        assert_eq!(&row[200 * 4..], &changed.rgba[source]);
    }
    Ok(())
}

fn exercise_posed_culling(
    renderer: &mut Renderer,
    fixture: &xrack_interpreter::Program,
) -> TestResult {
    use scene3d_model_ui::CullPoseScene;
    let generated = CullPoseScene::new();
    let weak = generated.as_weak();
    let dynamic = fixture.create("CullPoseScene")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut frame = |cull, morph, time| -> TestResult<_> {
        let mut generated = weak.upgrade().ok_or("missing posed scene")?;
        generated.set_cull(cull);
        generated.set_morph(morph);
        generated.set_time(time);
        dynamic.set("cull", Value::Bool(cull))?;
        dynamic.set("morph", Value::Bool(morph))?;
        dynamic.set("time", Value::Float(time))?;
        let before = renderer.scene_draw_stats();
        let mesh_uploads = renderer.scene_mesh_uploads();
        let image = aot.render(renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        let uploaded = renderer.scene_mesh_uploads() - mesh_uploads;
        assert_eq!(image.as_slice(), interpreted.render(renderer)?);
        Ok((
            image,
            [
                after.color_instances - before.color_instances,
                after.culled_instances - before.culled_instances,
                uploaded,
            ],
        ))
    };

    for morph in [false, true] {
        let (rest, submitted) = frame(true, morph, 0.0)?;
        assert_eq!(
            submitted,
            [0, 2, 0],
            "offscreen deformation skips color work and upload"
        );
        let (moved, submitted) = frame(true, morph, 1.0)?;
        assert_ne!(moved, rest);
        assert_eq!(
            submitted[..2],
            [1, 1],
            "animated bounds move into the frustum"
        );
        assert_eq!(frame(false, morph, 1.0)?.0, moved);
    }
    Ok(())
}

fn exercise_lod(renderer: &mut Renderer) -> TestResult {
    use lod_ui::LodDemo;
    const NAME: &str = "examples/puzzle3d/lod.xr";
    let source = std::fs::read_to_string(NAME)?;
    let mut preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let translations = preview.take_translation_catalog();
    let (program, _) = xrack_interpreter::Program::from_module_with_file_resources(
        &preview.module,
        NAME,
        translations,
    )?;

    #[derive(Clone, Copy)]
    struct View {
        distance: f64,
        orthographic: bool,
        full_detail: bool,
        bias: f64,
        factor: f64,
        instanced: bool,
        animated: bool,
        automatic: bool,
        time: f64,
        shadows: bool,
    }
    let base = View {
        distance: 4.0,
        orthographic: false,
        full_detail: false,
        bias: 1.0,
        factor: -1.0,
        instanced: false,
        animated: false,
        automatic: false,
        time: 0.0,
        shadows: false,
    };
    let generated = LodDemo::new();
    let weak = generated.as_weak();
    let dynamic = program.create("LodDemo")?;
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut frame = |view: View,
                     expected_view_z: Option<f64>,
                     expected_orbit_z: Option<f64>|
     -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing LOD scene")?;
        app.set_distance(view.distance);
        app.set_orthographic(view.orthographic);
        app.set_full_detail(view.full_detail);
        app.set_object_bias(view.bias);
        app.set_lod_factor(view.factor);
        app.set_instanced(view.instanced);
        app.set_animated(view.animated);
        app.set_automatic(view.automatic);
        app.set_time(view.time);
        app.set_shadows(view.shadows);
        for (name, value) in [
            ("distance", Value::Float(view.distance)),
            ("orthographic", Value::Bool(view.orthographic)),
            ("full-detail", Value::Bool(view.full_detail)),
            ("object-bias", Value::Float(view.bias)),
            ("lod-factor", Value::Float(view.factor)),
            ("instanced", Value::Bool(view.instanced)),
            ("animated", Value::Bool(view.animated)),
            ("automatic", Value::Bool(view.automatic)),
            ("time", Value::Float(view.time)),
            ("shadows", Value::Bool(view.shadows)),
        ] {
            dynamic.set(name, value)?;
        }

        for (name, hit, expected) in [
            ("camera-hit", app.get_camera_hit(), expected_view_z),
            ("orbit-hit", app.get_orbit_hit(), expected_orbit_z),
            (
                "world-hit",
                app.get_world_hit(),
                (!(view.automatic && view.animated && view.time == 1.0)).then_some(1.0),
            ),
        ] {
            assert_eq!(dynamic.get(name)?, Value::Hit3D(hit.clone()));
            assert_eq!(hit.valid, expected.is_some(), "{name} d={}", view.distance);
            if let Some(z) = expected {
                assert_eq!(hit.object_id, "target");
                assert_eq!(hit.row_index, 1, "replacement keeps the primary row");
                if view.automatic && !view.full_detail && name != "world-hit" {
                    assert!((0.82..=1.01).contains(&hit.position.z), "{name}: {hit:?}");
                    assert!((0.0..=1.0).contains(&hit.u) && (0.0..=1.0).contains(&hit.v));
                } else {
                    assert!((hit.position.z - z).abs() < 0.0001, "{name}: {hit:?}");
                }
            }
        }
        let expected: Vec<_> = expected_view_z
            .into_iter()
            .map(|_| app.get_camera_hit())
            .collect();
        assert_eq!(app.get_camera_hits(), expected);
        let Value::Hit3DList(actual) = dynamic.get("camera-hits")? else {
            return Err("missing typed view hits".into());
        };
        assert_eq!(actual.into_snapshot(), expected);

        let before = renderer.scene_draw_stats();
        let shadows = renderer.scene_shadow_passes();
        let uploads = renderer.scene_mesh_uploads();
        let image = aot.render(renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert_eq!(image.as_slice(), interpreted.render(renderer)?);
        if view.animated && !view.automatic && expected_view_z == Some(0.0) {
            let hit = app.get_camera_hit();
            assert!(
                hit.mesh_node_index >= 0,
                "the named camera chose the animated replacement"
            );
            let (node, _) = inspect(&aot)?;
            let Some(Value::SceneObjects(model)) = node.properties.get("model") else {
                return Err("missing LOD scene model".into());
            };
            let address = [hit.row_index, hit.mesh_node_index, hit.instance_index];
            let viewport = xrack_runtime::Point2D::new(
                xrack_runtime::Length(280.0),
                xrack_runtime::Length(252.0),
            );
            let selected = model.instance_camera("view", viewport, address);
            assert!(selected.valid);
            assert_eq!(&*selected.object_id, "target");
            assert!(
                !model.instance(address).valid,
                "the primitive primary has no imported nodes"
            );
        }
        if view.factor >= 0.0 && !view.full_detail {
            let (node, _) = inspect(&aot)?;
            let Some(Value::SceneObjects(model)) = node.properties.get("model") else {
                return Err("missing LOD scene model".into());
            };
            let camera = model
                .camera("view", [280.0, 252.0])
                .ok_or("missing view camera")?;
            let other = SceneCamera::new(0.0, 0.0, view.distance + 4.0, [320.0, 200.0])
                .ok_or("missing comparison camera")?;
            let selected = model.geometry(&camera);
            assert!(std::sync::Arc::ptr_eq(&selected, &model.geometry(&other)));
            if view.instanced && view.automatic && !view.animated {
                let addresses = selected
                    .iter()
                    .map(|object| object.imported_placement())
                    .collect::<Vec<_>>();
                assert_eq!(addresses, [Some((0, Some(0))), Some((0, Some(1)))]);
                assert_eq!(app.get_camera_hit().instance_index, 0);
                assert_eq!(app.get_world_hit().instance_index, 0);
            }
        }
        Ok([
            after.color_vertices - before.color_vertices,
            after.lod_objects - before.lod_objects,
            renderer.scene_shadow_passes() - shadows,
            renderer.scene_mesh_uploads() - uploads,
            after.color_draws - before.color_draws,
        ])
    };

    let sphere = Mesh3D::Sphere.vertices().len() as u64;
    let close = frame(base, Some(1.0), Some(1.0))?;
    assert_eq!(close[..2], [sphere, 0]);
    let middle = frame(
        View {
            distance: 12.0,
            ..base
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(
        middle[..2],
        [96, 1],
        "the authored middle mesh has 32 triangles"
    );
    let middle_move = frame(
        View {
            distance: 13.0,
            ..base
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(
        middle_move,
        [96, 1, 0, 0, 1],
        "same LOD retains its GPU mesh across camera motion"
    );
    let far = frame(
        View {
            distance: 40.0,
            ..base
        },
        Some(0.5),
        Some(0.5),
    )?;
    assert_eq!(far[..2], [36, 1]);
    let ortho = frame(
        View {
            distance: 40.0,
            orthographic: true,
            ..base
        },
        Some(1.0),
        Some(0.5),
    )?;
    assert_eq!(ortho[..2], [96, 1]);
    let named_full = frame(
        View {
            distance: 40.0,
            full_detail: true,
            ..base
        },
        Some(1.0),
        Some(0.5),
    )?;
    assert_eq!(
        named_full[..2],
        [sphere, 0],
        "bias zero disables this camera's LOD"
    );
    let object_full = frame(
        View {
            distance: 40.0,
            bias: 0.0,
            ..base
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(object_full[..2], [sphere, 0]);

    let manual = View {
        factor: 0.5,
        instanced: true,
        ..base
    };
    let middle_manual = frame(manual, Some(1.0), Some(1.0))?;
    assert_eq!(middle_manual[..2], [96, 1]);
    let same_level = frame(
        View {
            distance: 15.0,
            factor: 0.65,
            ..manual
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(same_level, [96, 1, 0, 0, 1]);
    let lowest_manual = frame(
        View {
            factor: 1.0,
            ..manual
        },
        Some(0.5),
        Some(0.5),
    )?;
    assert_eq!(lowest_manual[..2], [36, 1]);
    let primary_manual = frame(
        View {
            distance: 40.0,
            factor: 0.0,
            ..manual
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(primary_manual[..2], [sphere, 0]);
    let full_override = frame(
        View {
            full_detail: true,
            ..manual
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(full_override[..2], [sphere, 0]);

    let animated = View {
        distance: 12.0,
        animated: true,
        ..base
    };
    assert_eq!(frame(animated, Some(0.0), Some(0.0))?[1], 1);
    assert_eq!(
        frame(
            View {
                time: 1.0,
                ..animated
            },
            None,
            None
        )?[1],
        1
    );

    let low_shadow = frame(
        View {
            distance: 30.0,
            shadows: true,
            ..base
        },
        Some(0.5),
        Some(0.5),
    )?;
    let high_shadow = frame(
        View {
            distance: 30.0,
            shadows: true,
            full_detail: true,
            ..base
        },
        Some(1.0),
        Some(0.5),
    )?;
    assert_eq!(low_shadow[..2], [72, 1]);
    assert_eq!(high_shadow[..2], [sphere + 36, 0]);
    assert!(
        low_shadow[2] > 0 && high_shadow[2] > 0,
        "LOD changes invalidate the shadow caster"
    );
    let generated = View {
        distance: 40.0,
        automatic: true,
        bias: 4.0,
        ..base
    };
    let full = frame(
        View {
            full_detail: true,
            ..generated
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!([full[0], full[4]], [6624, 2]);
    let reduced = frame(generated, Some(1.0), Some(1.0))?;
    assert_eq!(
        [reduced[1], reduced[4]],
        [1, 2],
        "material sections remain separate"
    );
    assert!(reduced[0] * 2 < full[0], "full={full:?}, auto={reduced:?}");
    let reused = frame(
        View {
            distance: 42.0,
            ..generated
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(reused, [reduced[0], 1, 0, 0, 2]);
    let instanced = View {
        distance: 7.0,
        factor: 0.0,
        instanced: true,
        ..generated
    };
    let full_instanced = frame(instanced, Some(1.0), Some(1.0))?;
    assert_eq!([full_instanced[0], full_instanced[4]], [13248, 2]);
    let fixed = View {
        factor: 1.0,
        ..instanced
    };
    let fixed_generated = frame(fixed, Some(1.0), Some(1.0))?;
    assert_eq!([fixed_generated[1], fixed_generated[4]], [1, 2]);
    assert!(
        fixed_generated[0] * 2 < full_instanced[0],
        "{fixed_generated:?}"
    );
    let same_fixed = frame(
        View {
            distance: 8.0,
            ..fixed
        },
        Some(1.0),
        Some(1.0),
    )?;
    assert_eq!(same_fixed, [fixed_generated[0], 1, 0, 0, 2]);
    eprintln!(
        "Fixed LOD color vertex submissions: {} → {}",
        full_instanced[0], fixed_generated[0]
    );
    let animated_generated = View {
        animated: true,
        ..generated
    };
    assert_eq!(
        frame(animated_generated, Some(1.0), Some(1.0))?[0],
        reduced[0]
    );
    let moved = frame(
        View {
            time: 1.0,
            ..animated_generated
        },
        None,
        None,
    )?;
    assert_eq!(
        moved[..2],
        [reduced[0], 1],
        "the rigid rig keeps its animated transform"
    );
    eprintln!(
        "Automatic LOD color vertex submissions: {} → {}",
        full[0], reduced[0]
    );

    let mut deform_frame = |morph_only, full_detail, time| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing deformed LOD scene")?;
        app.set_deformation(true);
        app.set_automatic(true);
        app.set_animated(false);
        app.set_morph_only(morph_only);
        app.set_full_detail(full_detail);
        app.set_distance(40.0);
        app.set_object_bias(8.0);
        app.set_time(time);
        for (name, value) in [
            ("deformation", Value::Bool(true)),
            ("automatic", Value::Bool(true)),
            ("animated", Value::Bool(false)),
            ("morph-only", Value::Bool(morph_only)),
            ("full-detail", Value::Bool(full_detail)),
            ("distance", Value::Float(40.0)),
            ("object-bias", Value::Float(8.0)),
            ("time", Value::Float(time)),
        ] {
            dynamic.set(name, value)?;
        }
        let hit = app.get_camera_hit();
        let primary = app.get_world_hit();
        assert_eq!(dynamic.get("camera-hit")?, Value::Hit3D(hit.clone()));
        assert_eq!(dynamic.get("world-hit")?, Value::Hit3D(primary.clone()));
        assert_eq!(hit.object_id, "target");
        assert_eq!(hit.row_index, 1);
        assert!(hit.valid && primary.valid);
        let before = renderer.scene_draw_stats();
        let image = aot.render(renderer)?.to_vec();
        let after = renderer.scene_draw_stats();
        assert_eq!(image.as_slice(), interpreted.render(renderer)?);
        if full_detail {
            let (viewport, _) = inspect(&aot)?;
            let Some(Value::SceneObjects(objects)) = viewport.properties.get("model") else {
                return Err("missing deformed LOD models".into());
            };
            let mesh = &objects
                .iter()
                .find(|object| object.id == "target")
                .ok_or("missing deformed LOD target")?
                .mesh;
            let direct_bytes = (mesh.vertices().len() * xrack_runtime::scene3d::MeshVertex::STRIDE
                + size_of_val(mesh.tangents())
                + mesh.influences().map_or(0, size_of_val)
                + mesh
                    .morph_geometry()
                    .map_or(0, |morph| morph.packed_words().count() * size_of::<u32>()))
                as u64;
            let uploaded = after.mesh_vertex_bytes_uploaded - before.mesh_vertex_bytes_uploaded
                + after.mesh_index_bytes_uploaded
                - before.mesh_index_bytes_uploaded
                + after.mesh_deformation_bytes_uploaded
                - before.mesh_deformation_bytes_uploaded;
            assert_eq!(after.indexed_mesh_uploads - before.indexed_mesh_uploads, 1);
            assert!(
                uploaded > 0 && uploaded * 2 < direct_bytes,
                "{uploaded} vs {direct_bytes}"
            );
            eprintln!(
                "Imported LOD GPU geometry (morph only={morph_only}): {uploaded} bytes indexed vs {direct_bytes} bytes expanded"
            );
        } else if time == 0.0 {
            assert_eq!(after.indexed_mesh_uploads - before.indexed_mesh_uploads, 1);
        } else {
            assert_eq!(
                after.mesh_deformation_bytes_uploaded,
                before.mesh_deformation_bytes_uploaded
            );
        }
        Ok((
            hit,
            primary,
            image,
            after.color_vertices - before.color_vertices,
            after.lod_objects - before.lod_objects,
        ))
    };

    for morph_only in [true, false] {
        let (rest, _, rest_pixels, _, selected) = deform_frame(morph_only, false, 0.0)?;
        assert_eq!(selected, 1);
        let (full_hit, world, _, full, _) = deform_frame(morph_only, true, 1.0)?;
        let (reduced_hit, same_world, moved, reduced, selected) =
            deform_frame(morph_only, false, 1.0)?;
        assert_eq!(selected, 1);
        assert_eq!(full, 4608);
        assert!(reduced * 2 < full, "full={full}, reduced={reduced}");
        assert_ne!(rest_pixels, moved);
        assert!(full_hit.position.z > rest.position.z + 0.27);
        assert!((full_hit.position.z - reduced_hit.position.z).abs() < 0.15);
        assert_eq!(
            world, same_world,
            "world queries retain the high-detail deformed mesh"
        );
        eprintln!(
            "Deformed LOD (morph only={morph_only}) color vertex submissions: {full} → {reduced}"
        );
    }

    let error = xrack_compiler::prepare_preview(
        &source.replace(", auto-lod", ", unsupported-mode"),
        NAME,
        Default::default(),
    )
    .err()
    .ok_or("unknown mesh URL option was accepted")?;
    assert!(error.to_string().contains("expected `auto-lod`"), "{error}");
    Ok(())
}

#[test]
fn scene_morph_weights_require_a_float_record() -> TestResult {
    let body = r#"
        struct Pose { extend: WEIGHT_TYPE }
        struct Actor {
            id: string, mesh: string, x: float, y: float, z: float,
            scale-x: float, scale-y: float, scale-z: float, yaw: float, pitch: float,
            roll: float, color: color, morph-weights: FIELD_TYPE,
        }
        export component Test inherits Window {
            property <[Actor]> room: [];
            BODY
        }
    "#;
    let use_sites = [
        "SceneView3D { model: root.room; }",
        "out property <string> hit: Scene3D.pick(root.room, 0, 0, 8, Point2D.new(200px, 200px), Point2D.new(100px, 100px));",
    ];
    for use_site in use_sites {
        let doc = body.replace("BODY", use_site);
        for (field, weight) in [("float", "float"), ("Pose", "bool")] {
            let invalid = doc
                .replace("FIELD_TYPE", field)
                .replace("WEIGHT_TYPE", weight);
            let error = xrack_compiler::compile(&invalid, "bad_morph.xr")
                .err()
                .ok_or("accepted invalid override schema")?;
            assert!(
                error
                    .to_string()
                    .contains("requires a morph-weights record with float fields"),
                "{error}",
            );
        }
        let valid = doc
            .replace("FIELD_TYPE", "Pose")
            .replace("WEIGHT_TYPE", "float");
        let preview =
            xrack_compiler::prepare_preview(&valid, "morph_schema.xr", Default::default())?;
        let mlir = preview.module.to_mlir();
        let malformed = mlir.replacen(
            r"\22extend\22,\22ty\22:\22float\22",
            r"\22extend\22,\22ty\22:\22bool\22",
            1,
        );
        assert!(
            malformed != mlir,
            "missing float field in raw record declaration"
        );
        let error = xrack_interpreter::load_mlir(&malformed)
            .err()
            .ok_or("accepted invalid override schema in raw MLIR")?;
        assert!(
            error
                .to_string()
                .contains("requires a morph-weights record with float fields"),
            "{error}",
        );
    }
    Ok(())
}

#[test]
fn typed_scene_lights_require_their_fields() -> TestResult {
    let scene = r#"
        struct Lamp { kind: string }
        export component BadLight inherits Window {
            property <[Lamp]> lamps: [];
            SceneView3D { lights: root.lamps; }
        }
    "#;
    let missing = xrack_compiler::compile(scene, "bad_scene_lights.xr")
        .err()
        .ok_or("accepted a light without its fields")?;
    assert!(
        missing
            .to_string()
            .contains("3D lights record `Lamp` requires a float field named `x`"),
        "{missing}"
    );
    let complete = scene.replace("kind: string", "kind: string, x: float, y: float, z: float, yaw: float, elevation: float, intensity: float, color: color, range: float, inner-angle: float, outer-angle: float");
    let bad_flag = complete.replace(
        "outer-angle: float",
        "outer-angle: float, casts-shadow: string",
    );
    let diagnostic = xrack_compiler::compile(&bad_flag, "bad_shadow_flag.xr")
        .err()
        .ok_or("accepted non-boolean casts-shadow")?;
    assert!(
        diagnostic
            .to_string()
            .contains("requires a bool field named `casts-shadow`"),
        "{diagnostic}"
    );
    let bad_parent = complete.replace("outer-angle: float", "outer-angle: float, parent: bool");
    let diagnostic = xrack_compiler::compile(&bad_parent, "bad_light_parent.xr")
        .err()
        .ok_or("accepted non-string light parent")?;
    assert!(
        diagnostic
            .to_string()
            .contains("requires a string field named `parent`")
    );
    let module = xrack_compiler::prepare_preview(&complete, "scene_lights.xr", Default::default())?;
    let raw = module.module.to_mlir();
    let bad = raw.replacen(r"\22intensity\22", r"\22power\22", 1);
    assert_ne!(bad, raw);
    let diagnostic = xrack_interpreter::load_mlir(&bad)
        .err()
        .ok_or("accepted a raw light record without intensity")?;
    assert!(
        diagnostic.to_string().contains("field named intensity"),
        "{diagnostic}"
    );
    Ok(())
}

#[test]
fn typed_scene_requires_the_same_fields_for_drawing_and_picking() -> TestResult {
    let schema = "id: string, x: float, y: float, z: float, scale-x: float, scale-y: float, scale-z: float, yaw: float, pitch: float, roll: float, color: color";
    let body = r#"
        struct Bad { SCHEMA }
        export component Test inherits Window {
            property <[Bad]> room: [];
            BODY
        }
    "#;
    for use_site in [
        "SceneView3D { model: root.room; }",
        "out property <string> hit: Scene3D.pick(root.room, 0, 0, 8, Point2D.new(200px, 200px), Point2D.new(100px, 100px));",
    ] {
        let source = body.replace("SCHEMA", schema).replace("BODY", use_site);
        let error = xrack_compiler::compile(&source, "bad_scene_model.xr")
            .err()
            .expect("invalid model should produce a diagnostic");
        assert!(
            error
                .to_string()
                .contains("requires a string field named `mesh`"),
            "{error}"
        );
    }
    let complete = body
        .replace("SCHEMA", &format!("{schema}, mesh: string, parent: bool"))
        .replace("BODY", "SceneView3D { model: root.room; }");
    let invalid_parent = xrack_compiler::compile(&complete, "bad_scene_parent.xr")
        .err()
        .ok_or("accepted a non-string scene parent")?;
    assert!(
        invalid_parent
            .to_string()
            .contains("requires a string field named `parent`")
    );
    let preview = xrack_compiler::prepare_preview(
        include_str!("scene3d_model.xr"),
        "tests/scene3d_model.xr",
        Default::default(),
    )?;
    let mlir = preview.module.to_mlir();
    let invalid = mlir.replacen(r"\22mesh\22", r"\22mesh-key\22", 1);
    assert_ne!(invalid, mlir);
    let error = xrack_interpreter::load_mlir(&invalid)
        .err()
        .ok_or("invalid raw scene model was accepted")?;
    let error = error.to_string();
    assert!(
        error.contains("mesh") && error.contains("geometry record"),
        "{error}"
    );
    Ok(())
}

#[test]
fn offscreen_points_round_trip_and_invalid_inputs_use_fallback() -> TestResult {
    use xrack_runtime::{Length, Point2D, Point3D, ProjectedPoint3D, scene3d};
    let viewport = Point2D::new(Length(300.0), Length(200.0));
    let camera = SceneCamera::new(25.0, -20.0, 9.0, [300.0, 200.0]).ok_or("orbit camera")?;
    let world = Point3D::new(15.0, 1.0, -4.0);
    let projected = camera.project(world);
    assert!(projected.valid && !projected.inside);
    let restored = camera
        .unproject(projected.point, projected.depth)
        .ok_or("offscreen inverse")?;
    assert!((restored.x - world.x).abs() < 1e-4);
    assert!((restored.y - world.y).abs() < 1e-4);
    assert!((restored.z - world.z).abs() < 1e-4);

    for world in [
        Point3D::new(0.0, 0.0, 9.0),
        Point3D::new(f64::INFINITY, 0.0, 0.0),
    ] {
        assert_eq!(
            scene3d::project(0.0, 0.0, 8.0, viewport, world),
            ProjectedPoint3D::default()
        );
    }
    assert_eq!(
        scene3d::unproject(0.0, 0.0, 8.0, viewport, projected.point, 0.0, world),
        world
    );
    assert_eq!(
        scene3d::unproject(
            0.0,
            0.0,
            8.0,
            Point2D::default(),
            projected.point,
            1.0,
            world
        ),
        world
    );
    Ok(())
}

#[test]
fn rigid_bvh_respects_masking_and_coplanar_draw_order() -> TestResult {
    use xrack_runtime::{
        Color, Point3D, Vector3D,
        scene3d::{MeshAlphaMode, MeshMaterial, MeshPart, MeshVertex, SceneObject, ray_hit},
    };

    for layer_mode in [MeshAlphaMode::Opaque, MeshAlphaMode::Blend] {
        let mut vertices = Vec::new();
        let mut parts = Vec::new();
        let mut triangle = |x, z, u, alpha, material| {
            let start = vertices.len();
            vertices.extend(
                [[-0.5, -0.5], [0.5, -0.5], [0.0, 0.5]].map(|[dx, y]| MeshVertex {
                    position: [x + dx, y, z],
                    normal: [0.0, 0.0, 1.0],
                    uv: [u, 0.5],
                    uv1: [0.0; 2],
                    color: [1.0, 1.0, 1.0, alpha],
                }),
            );
            parts.push(MeshPart {
                vertices: start..start + 3,
                material,
                volume_inverse: None,
            });
        };
        triangle(0.0, 0.125, 0.0, 0.0, 0);
        triangle(0.0, 0.0, 0.125, 1.0, 1);
        for x in 1..80 {
            triangle(x as f32 * 2.0, 0.0, 1.0, 1.0, 2);
        }
        triangle(0.0, 0.0, 0.375, 1.0, 2);
        triangle(0.0, 0.0, 0.75, 1.0, 2);
        let materials =
            [MeshAlphaMode::Mask(0.5), MeshAlphaMode::Blend, layer_mode].map(|alpha_mode| {
                MeshMaterial {
                    shader: None,
                    channels: Default::default(),
                    vertex_colors: true,
                    vertex_masks: Default::default(),
                    cull_mode: Default::default(),
                    depth_draw_mode: Default::default(),
                    blend_mode: Default::default(),
                    base_color: [1.0; 4],
                    alpha_mode,
                    base_texture: None,
                    base_item: None,
                    pbr: None,
                }
            });
        let mesh = Mesh3D::imported(vertices, Vec::new(), parts, materials.into())?;
        // An anonymous surface is still a valid hit and can block another object.
        let object = SceneObject::from_mesh(
            "".into(),
            mesh,
            [0.0; 3],
            [1.0; 3],
            [0.0; 3],
            Color(0xffffffff),
        )
        .ok_or("invalid test mesh")?;
        let objects = [object];
        let origin = Point3D::new(0.0, 0.0, 1.0);
        let direction = Vector3D::new(0.0, 0.0, -1e-80);
        let hit = ray_hit(&objects, origin, direction, 2.0, None);
        assert!(hit.valid && hit.object_id.is_empty());
        assert_eq!(hit.distance, 1.0);
        assert_eq!(hit.local_normal, Vector3D::new(0.0, 0.0, 1.0));
        assert_eq!(
            hit.u,
            if layer_mode == MeshAlphaMode::Opaque {
                0.375
            } else {
                0.75
            }
        );
        assert!(!ray_hit(&objects, origin, direction, 0.5, None).valid);
        assert!(!ray_hit(&objects, origin, Vector3D::default(), 2.0, None).valid);
    }
    Ok(())
}

#[test]
fn all_hits_keep_the_same_coplanar_priority_as_single_picking() -> TestResult {
    use xrack_runtime::{
        Color,
        scene3d::{
            MeshAlphaMode, MeshMaterial, MeshPart, MeshVertex, Ray3D, SceneObject, raycast,
            raycast_all,
        },
    };

    let layer = |id: &str, alpha_mode| -> TestResult<SceneObject> {
        let vertices =
            [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]].map(|position| MeshVertex {
                position,
                normal: [0.0, 0.0, 1.0],
                uv: [0.0; 2],
                uv1: [0.0; 2],
                color: [1.0; 4],
            });
        let mesh = Mesh3D::imported(
            vertices.into(),
            Vec::new(),
            vec![MeshPart {
                vertices: 0..3,
                material: 0,
                volume_inverse: None,
            }],
            vec![MeshMaterial {
                shader: None,
                channels: Default::default(),
                vertex_colors: true,
                vertex_masks: Default::default(),
                cull_mode: Default::default(),
                depth_draw_mode: Default::default(),
                blend_mode: Default::default(),
                base_color: [1.0; 4],
                alpha_mode,
                base_texture: None,
                base_item: None,
                pbr: None,
            }],
        )?;
        SceneObject::from_mesh(
            id.into(),
            mesh,
            [0.0; 3],
            [1.0; 3],
            [0.0; 3],
            Color(0xffffffff),
        )
        .ok_or_else(|| "invalid coplanar test layer".into())
    };
    let scene = [
        layer("blend-first", MeshAlphaMode::Blend)?,
        layer("opaque-first", MeshAlphaMode::Opaque)?,
        layer("opaque-second", MeshAlphaMode::Opaque)?,
        layer("blend-last", MeshAlphaMode::Blend)?,
    ];
    let ray = Ray3D::new([0.0, 0.0, 2.0], [0.0, 0.0, -1.0]).ok_or("invalid test ray")?;
    let hits = raycast_all(&scene, ray, 3.0);
    assert_eq!(
        hits.iter().map(|hit| hit.object_id).collect::<Vec<_>>(),
        ["opaque-first", "opaque-second", "blend-last", "blend-first"],
    );
    assert_eq!(
        raycast(&scene, ray, 3.0).map(|hit| hit.object_id),
        Some(hits[0].object_id)
    );
    Ok(())
}

#[test]
fn nearest_geometry_can_occlude_a_pick() -> TestResult {
    use xrack_runtime::{
        Color, Length, Point2D,
        scene3d::{SceneObject, pick_objects},
    };
    let cube = |id, z| {
        SceneObject::cuboid(id, [0.0, 0.0, z], [1.0; 3], [0.0; 3], Color(0x00ff00ff))
            .ok_or("valid box")
    };
    let viewport = Point2D::new(Length(200.0), Length(200.0));
    let center = Point2D::new(Length(100.0), Length(100.0));
    assert_eq!(
        pick_objects(
            &[cube("back", 0.0)?, cube("front", 2.0)?],
            0.0,
            0.0,
            8.0,
            viewport,
            center
        ),
        "front"
    );
    assert_eq!(
        pick_objects(
            &[cube("", 2.0)?, cube("back", 0.0)?],
            0.0,
            0.0,
            8.0,
            viewport,
            center
        ),
        ""
    );
    Ok(())
}

#[test]
fn raycast_reports_world_distance_and_respects_the_rotated_mesh() -> TestResult {
    use xrack_runtime::{
        Color,
        scene3d::{Ray3D, SceneObject, raycast},
    };

    let tint = Color(0xffff_ffff);
    let closed = SceneObject::cuboid("door", [0.5, 0.0, 0.0], [4.0, 1.0, 0.2], [0.0; 3], tint)
        .ok_or("valid closed door")?;
    let opened = SceneObject::cuboid(
        "door",
        [0.5, 0.0, 0.0],
        [4.0, 1.0, 0.2],
        [90.0, 0.0, 0.0],
        tint,
    )
    .ok_or("valid opened door")?;
    let ball = || SceneObject::sphere("orb", [0.0, 0.0, -2.0], 1.0, tint).ok_or("valid orb");
    let ray = Ray3D::new([0.0, 0.0, 3.0], [0.0, 0.0, -2.0]).ok_or("valid ray")?;
    let closed_room = [closed, ball()?];
    let hit = raycast(&closed_room, ray, 10.0).ok_or("closed door blocks the ray")?;
    assert_eq!(hit.object_id, "door");
    assert!((hit.distance - 2.9).abs() < 1e-5, "{hit:?}");
    assert!((hit.position[2] - 0.1).abs() < 1e-5, "{hit:?}");
    assert_eq!(hit.normal, [0.0, 0.0, 1.0]);

    let opened_room = [opened, ball()?];
    assert!(raycast(&opened_room, ray, 3.5).is_none());
    let hit = raycast(&opened_room, ray, 10.0).ok_or("open door exposes the sphere")?;
    assert_eq!(hit.object_id, "orb");
    assert!((hit.distance - 4.0).abs() < 1e-5, "{hit:?}");
    assert!(hit.normal[2] > 0.99, "{hit:?}");

    let corner = Ray3D::new([0.85, 0.85, 3.0], [0.0, 0.0, -1.0]).ok_or("valid ray")?;
    assert!(raycast(&opened_room, corner, 10.0).is_none());
    Ok(())
}

#[test]
fn gltf_material_variants_keep_rendering_and_picking_in_sync() -> TestResult {
    use material_variants_ui::MaterialVariantsDemo;

    const NAME: &str = "examples/puzzle3d/material_variants.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialVariantsDemo")?;
    let app = MaterialVariantsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    struct Frame {
        pixels: [[u8; 4]; 3],
        pick: String,
        counters: [u64; 3],
    }
    let mut render = |variant: &str, recolor: bool| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing material variant demo")?;
        app.set_variant(variant.into());
        app.set_recolor(recolor);
        dynamic.set("variant", Value::String(variant.into()))?;
        dynamic.set("recolor", Value::Bool(recolor))?;
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png, interpreter.render(&mut preview_gpu)?, "{variant}");
        let pick = app.get_shell_hit();
        assert_eq!(dynamic.get("shell-hit")?, Value::String(pick.clone()));
        let counters = |renderer: &Renderer| {
            [
                renderer.scene_mesh_uploads(),
                renderer.scene_instance_uploads(),
                renderer.scene_shadow_passes(),
            ]
        };
        assert_eq!(counters(&gpu), counters(&preview_gpu));
        let raster = xrack_assets::decode_png(&png)?;
        let pixels = raster.rgba.as_chunks::<4>().0;
        Ok(Frame {
            pixels: [183, 317, 451].map(|x| pixels[162 * 634 + x]),
            pick,
            counters: counters(&gpu),
        })
    };

    let default = render("", false)?;
    assert_eq!(default.pick, "model");
    assert!(default.counters[2] > 0);
    for selector in ["unknown", "Unmapped", "#4", "#100"] {
        assert_eq!(
            render(selector, false)?.pixels,
            default.pixels,
            "{selector}"
        );
    }
    let copper = render("Copper", false)?;
    assert_ne!(copper.pixels[0], default.pixels[0]);
    assert_eq!(copper.pixels[1..], default.pixels[1..]);
    assert_eq!(copper.counters, default.counters);
    assert_eq!(render("#0", false)?.pixels, copper.pixels);

    let recolored = render("#0", true)?;
    assert_ne!(recolored.pixels[0], copper.pixels[0]);
    assert_eq!(recolored.pixels[1..], copper.pixels[1..]);
    assert_eq!(recolored.counters, copper.counters);

    let cutout = render("Cutout", false)?;
    assert_eq!(cutout.pick, "wall");
    assert_eq!(cutout.pixels[1..], copper.pixels[1..]);
    assert_eq!(cutout.counters[..2], copper.counters[..2]);
    assert!(cutout.counters[2] > copper.counters[2]);

    let uv0 = render("UV0", false)?;
    let uv1 = render("UV1", false)?;
    assert_eq!(uv0.pick, "model");
    assert_eq!(uv1.pick, "model");
    assert!(uv0.pixels[0][1] > uv0.pixels[0][2]);
    assert!(uv1.pixels[0][2] > uv1.pixels[0][1]);
    assert_eq!(uv0.pixels[1..], uv1.pixels[1..]);
    assert_eq!(
        uv0.counters[0], cutout.counters[0],
        "the source mesh retains TEXCOORD_0"
    );
    assert_eq!(
        uv1.counters[0],
        uv0.counters[0] + 1,
        "a different base UV slot uses its own stream"
    );
    assert_eq!(render("UV0", false)?.counters[0], uv1.counters[0]);
    Ok(())
}

#[test]
fn gltf_gpu_instancing_shares_meshes_and_keeps_animation_picking_and_culling_in_sync() -> TestResult
{
    use gpu_instancing_ui::GpuInstancingDemo;

    const NAME: &str = "examples/puzzle3d/gpu_instancing.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("GpuInstancingDemo")?;
    let generated = GpuInstancingDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut render = |time, blue| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing GPU instancing demo")?;
        app.set_time(time);
        app.set_blue(blue);
        dynamic.set("time", Value::Float(time))?;
        dynamic.set("blue", Value::Bool(blue))?;
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        let after = gpu.scene_draw_stats();
        assert_eq!(png.as_slice(), interpreted.render(&mut preview_gpu)?);
        let expected = if time == 0.0 { "triangles" } else { "" };
        assert_eq!(app.get_left_hit(), expected);
        assert_eq!(dynamic.get("left-hit")?, Value::String(expected.into()));
        let [node, instance] = if time == 0.0 { [1, 0] } else { [-1, -1] };
        assert_eq!(app.get_left_node(), node);
        assert_eq!(app.get_left_instance(), instance);
        assert_eq!(dynamic.get("left-node")?, Value::Int(node));
        assert_eq!(dynamic.get("left-instance")?, Value::Int(instance));
        assert!(
            app.get_base_hit().is_empty(),
            "the source mesh has no extra draw"
        );
        assert_eq!(dynamic.get("base-hit")?, Value::String(String::new()));
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
            return Err("missing instanced model".into());
        };
        assert_eq!(model.len(), 33);
        assert_eq!(model[0].imported_placement(), Some((1, Some(0))));
        assert_eq!(model[31].imported_placement(), Some((1, Some(31))));
        assert_eq!(model[32].imported_placement(), Some((2, None)));
        let position = |index: usize| model[index].transform_rows().map(|row| row[3]);
        assert_eq!(position(0), [-2.0 + time as f32, 0.0, 0.0]);
        assert_eq!(position(32), [1.0 + time as f32, 1.0, 0.0]);
        let rot = model[1].transform_rows();
        assert!(rot[0][0].abs() < 0.001 && (rot[0][1] + 2.0).abs() < 0.001);
        assert_eq!(
            model[3].transform_rows()[1][1],
            2.0,
            "sparse instance scale"
        );
        let (Mesh3D::Imported(first), Mesh3D::Imported(next)) = (&model[0].mesh, &model[1].mesh)
        else {
            return Err("missing imported triangle mesh".into());
        };
        assert!(
            std::sync::Arc::ptr_eq(first, next),
            "instances share the styled mesh"
        );
        assert_eq!(model[0].mesh.vertices().len(), 3);

        let draws = after.color_draws - before.color_draws;
        assert!(
            (1..=2).contains(&draws),
            "shared mesh batched into {draws} draws"
        );
        assert_eq!(after.color_instances - before.color_instances, 4);
        assert_eq!(after.culled_instances - before.culled_instances, 29);
        Ok((png, gpu.scene_mesh_uploads()))
    };

    let (original, uploads) = render(0.0, false)?;
    let (moved, moved_uploads) = render(1.0, false)?;
    assert_ne!(original, moved);
    assert_eq!(uploads, moved_uploads);
    let (blue, blue_uploads) = render(1.0, true)?;
    assert_ne!(moved, blue);
    assert_eq!(moved_uploads, blue_uploads);
    Ok(())
}

#[test]
fn gltf_instance_channels_survive_culling_and_apply_row_overrides() -> TestResult {
    use gltf_instance_data_ui::GltfInstanceDataDemo;
    use xrack_runtime::{Color, Length, Point2D, Point3D, Vector3D};

    const NAME: &str = "examples/puzzle3d/gltf_instance_data.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("GltfInstanceDataDemo")?;
    let generated = GltfInstanceDataDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut frame = |shift, tinted, query_x, indexed| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing glTF instance data demo")?;
        let point = Point2D::new(Length(query_x), Length(130.0));
        app.set_shift(shift);
        app.set_tinted(tinted);
        app.set_indexed(indexed);
        app.set_pointer(point);
        for (name, value) in [
            ("shift", Value::Float(shift)),
            ("tinted", Value::Bool(tinted)),
            ("indexed", Value::Bool(indexed)),
            ("pointer", Value::Point2D(point)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image, interpreted.render(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        assert_eq!(after.color_draws - before.color_draws, 2);
        assert_eq!(after.color_instances - before.color_instances, 8);
        assert_eq!(after.culled_instances - before.culled_instances, 2);

        let hit = app.get_result();
        assert_eq!(dynamic.get("result")?, Value::Hit3D(hit.clone()));
        let selected = app.get_selected();
        for (property, instance) in [
            ("selected", selected.clone()),
            ("primary-placement", app.get_primary_placement()),
            ("orbit-placement", app.get_orbit_placement()),
        ] {
            assert_eq!(dynamic.get(property)?, Value::Instance3D(instance.clone()));
            assert_eq!(instance, selected);
        }
        assert_eq!(selected.valid, hit.valid);
        if selected.valid {
            assert_eq!(
                (
                    &*selected.object_id,
                    selected.row_index,
                    selected.mesh_node_index,
                    selected.instance_index
                ),
                ("tiles", 1, 0, 2)
            );
            assert_eq!(selected.origin, Point3D::new(shift, 0.0, 0.0));
            assert_eq!(selected.axis_x, Vector3D::new(1.0, 0.0, 0.0));
            assert_eq!(selected.axis_y, Vector3D::new(0.0, 1.0, 0.0));
            assert_eq!(selected.axis_z, Vector3D::new(0.0, 0.0, 1.0));
            assert_eq!(selected.color, Color(0x00ff00ff));
            assert_eq!(
                [
                    selected.data_x,
                    selected.data_y,
                    selected.data_z,
                    selected.data_w
                ],
                [0.5, 1.0, 0.25, f64::from(0.8f32)]
            );
        } else {
            assert_eq!(selected.instance_index, -1);
        }
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
            return Err("missing glTF instance data model".into());
        };
        assert_eq!(model.len(), 5);
        let offscreen =
            model.instance_camera("eye", Point2D::new(Length(348.0), Length(260.0)), [1, 0, 1]);
        assert!(offscreen.valid, "unchanged by viewport culling");
        assert_eq!(offscreen.data_x, 8.0);
        assert_eq!(model[0].imported_placement(), Some((0, Some(0))));
        assert_eq!(model[2].imported_placement(), Some((0, Some(2))));
        assert_eq!(model[0].instance_data(), [0.0, 0.25, -0.5, 0.0]);
        assert_eq!(model[1].instance_data(), [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(model[2].instance_data(), [0.5, 1.0, 0.25, 0.8]);
        assert_eq!(model[4].color.0 as u8, 0);
        assert_eq!(model[2].color, Color(0x00ff00ff));
        let red = if tinted { 0x5d0000ff } else { 0xbc0000ff };
        assert_eq!(model[0].color, Color(red));
        let (Mesh3D::Imported(first), Mesh3D::Imported(last)) = (&model[0].mesh, &model[4].mesh)
        else {
            return Err("missing instanced geometry".into());
        };
        assert!(std::sync::Arc::ptr_eq(first, last));
        Ok((
            xrack_assets::decode_png(&image)?,
            hit,
            gpu.scene_mesh_uploads(),
            selected,
        ))
    };
    let pixel = |image: &xrack_assets::Raster, x, y| -> [u8; 3] {
        let offset = 4 * (y * image.size[0] as usize + x);
        std::array::from_fn(|channel| image.rgba[offset + channel])
    };

    let (original, hit, uploads, frozen) = frame(0.0, false, 174.0, false)?;
    assert_eq!(
        (hit.row_index, hit.mesh_node_index, hit.instance_index),
        (1, 0, 2)
    );
    let background = pixel(&original, 25, 65);
    assert_eq!(pixel(&original, 66, 166), [188, 0, 0]);
    assert_eq!(pixel(&original, 414, 166), [188, 0, 0]);
    assert_eq!(pixel(&original, 186, 166), background);
    assert_eq!(pixel(&original, 186, 134), [0, 225, 0]);
    assert_eq!(pixel(&original, 534, 166), [0, 255, 0]);
    assert_eq!(pixel(&original, 306, 166), [0, 0, 188]);
    assert_eq!(pixel(&original, 654, 166), [0, 0, 255]);
    assert_eq!(pixel(&original, 246, 166), background);

    let (_, masked, _, _) = frame(0.0, false, 234.0, false)?;
    assert!(!masked.valid, "zero imported alpha fails the visible mask");
    let (shifted, moved, reused, current) = frame(1.0, true, 214.0, false)?;
    assert_eq!(moved.instance_index, 2);
    assert_eq!(uploads, reused);
    assert_eq!(frozen.origin.x, 0.0);
    assert_eq!(current.origin.x, 1.0);
    assert_eq!(pixel(&shifted, 106, 166), [93, 0, 0]);
    assert_eq!(pixel(&shifted, 226, 134), [0, 225, 0]);
    let (indexed, hit, _, _) = frame(0.0, false, 174.0, true)?;
    assert_eq!(hit.instance_index, 2);
    assert_eq!(pixel(&indexed, 186, 106), [188, 0, 0]);
    assert_eq!(pixel(&indexed, 306, 118), [225, 0, 255]);
    let [red, green, blue] = pixel(&indexed, 66, 214);
    assert_eq!([red, blue], [0, 0]);
    assert!(
        green > 220,
        "vertex input needs the instance's world translation"
    );
    assert_eq!(pixel(&indexed, 66, 166), background);
    Ok(())
}

#[test]
fn authored_instance_tables_share_the_prototype_and_keep_row_identity() -> TestResult {
    use instance_table_ui::InstanceTableDemo;
    use xrack_runtime::Color;

    const NAME: &str = "examples/puzzle3d/instance_table.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("InstanceTableDemo")?;
    let generated = InstanceTableDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct Frame {
        shift: f64,
        color: Color,
        pickable: bool,
        far: bool,
        imported: bool,
        phase: f64,
    }
    let original = Frame {
        shift: 0.0,
        color: Color(0xed9768ff),
        pickable: true,
        far: false,
        imported: false,
        phase: 0.0,
    };
    let mut render = |frame: Frame| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing authored instance demo")?;
        app.set_shift(frame.shift);
        app.set_left_color(frame.color);
        app.set_left_pickable(frame.pickable);
        app.set_far(frame.far);
        app.set_imported(frame.imported);
        app.set_phase(frame.phase);
        for (property, value) in [
            ("shift", Value::Float(frame.shift)),
            ("left-color", Value::Color(frame.color)),
            ("left-pickable", Value::Bool(frame.pickable)),
            ("far", Value::Bool(frame.far)),
            ("imported", Value::Bool(frame.imported)),
            ("phase", Value::Float(frame.phase)),
        ] {
            dynamic.set(property, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        let after = gpu.scene_draw_stats();
        assert_eq!(png.as_slice(), interpreted.render(&mut preview_gpu)?);
        let hit = app.get_left_hit();
        assert_eq!(Value::Hit3D(hit.clone()), dynamic.get("left-hit")?);

        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
            return Err("missing projected instance table".into());
        };
        let stride = if frame.imported { 33 } else { 1 };
        assert_eq!(model.len(), 3 * stride);
        assert_eq!(model[0].id, "left");
        assert_eq!(model[stride].id, "right");
        assert_eq!(model[2 * stride].id, "remote");
        let (Mesh3D::Imported(left), Mesh3D::Imported(right)) =
            (&model[0].mesh, &model[stride].mesh)
        else {
            return Err("missing shared styled mesh".into());
        };
        assert!(std::sync::Arc::ptr_eq(left, right));
        assert_eq!(model[0].color, frame.color);
        assert_eq!(model[stride].color, Color(0x74bfe9ff));
        let left_origin = model[0].transform_rows().map(|row| row[3]);
        assert!(
            (left_origin[0]
                - (-1.3
                    + if frame.imported {
                        -2.0 + frame.phase as f32
                    } else {
                        0.0
                    }))
            .abs()
                < 0.001
        );
        assert_eq!(left_origin[1], frame.shift as f32);

        if !frame.imported {
            use xrack_runtime::{Point3D, Vector3D, scene3d::ray_hit};
            for (id, row, x, y) in [("right", 5, 1.3, frame.shift), ("remote", 6, 60.0, 0.0)] {
                let ray = ray_hit(
                    model,
                    Point3D::new(x, y, 8.0),
                    Vector3D::new(0.0, 0.0, -1.0),
                    16.0,
                    None,
                );
                assert_eq!((&*ray.object_id, ray.row_index), (id, row));
            }
            assert!(app.get_center_hit().is_empty());
            let wanted_hit = frame.pickable && frame.shift == 0.0 && !frame.far;
            assert_eq!(hit.valid, wanted_hit);
            if wanted_hit {
                assert_eq!((&*hit.object_id, hit.row_index), ("left", 4));
            }
            let camera = model
                .camera("eye", [640.0, 320.0])
                .ok_or("missing named camera")?;
            let selected = model.geometry(&camera);
            assert_eq!(selected[0].id, "left");
            assert_eq!(
                selected[0].mesh.vertices().len() == model[0].mesh.vertices().len(),
                !frame.far
            );
            assert_eq!(after.color_instances - before.color_instances, 2);
            assert_eq!(after.color_draws - before.color_draws, 1);
        }
        Ok((png, gpu.scene_mesh_uploads()))
    };

    let (base, uploads) = render(original)?;
    let (tinted, after_color) = render(Frame {
        color: Color(0xade076ff),
        ..original
    })?;
    assert_ne!(base, tinted);
    assert_eq!(uploads, after_color);
    let (moved, after_move) = render(Frame {
        shift: 1.0,
        ..original
    })?;
    assert_ne!(base, moved);
    assert_eq!(uploads, after_move);
    let (unpickable, after_pick) = render(Frame {
        pickable: false,
        ..original
    })?;
    assert_eq!(base, unpickable);
    assert_eq!(uploads, after_pick);
    render(Frame {
        far: true,
        ..original
    })?;
    let (rig, rig_uploads) = render(Frame {
        imported: true,
        ..original
    })?;
    let (animated, animated_uploads) = render(Frame {
        imported: true,
        phase: 1.0,
        ..original
    })?;
    assert_ne!(rig, animated);
    assert_eq!(rig_uploads, animated_uploads);
    Ok(())
}

#[test]
fn file_instance_tables_share_xml_and_qtir_placements_and_prefixes() -> TestResult {
    use file_instances_ui::FileInstancesDemo;
    use xrack_runtime::{Color, Point3D, Vector3D};

    const NAME: &str = "examples/puzzle3d/file_instances.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("FileInstancesDemo")?;
    let generated = FileInstancesDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut frame = |binary, limit: i64, offset| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing file instances demo")?;
        app.set_binary(binary);
        app.set_limit(limit);
        app.set_offset(offset);
        for (name, value) in [
            ("binary", Value::Bool(binary)),
            ("limit", Value::Int(limit)),
            ("offset", Value::Float(offset)),
        ] {
            dynamic.set(name, value)?;
        }
        let table = app.get_table();
        assert!(table.is_loaded() && table.count() == 3);
        assert_eq!(dynamic.get("table")?, Value::InstanceTable3D(table.clone()));
        let source = app.get_source_instance();
        assert_eq!(
            dynamic.get("source-instance")?,
            Value::Instance3D(source.clone())
        );
        assert!(source.valid);
        assert_eq!(
            (
                source.row_index,
                source.mesh_node_index,
                source.instance_index
            ),
            (-1, -1, 2)
        );
        assert_eq!(source.origin, Point3D::new(2.0, 0.0, 0.0));
        assert_eq!(source.axis_x, Vector3D::new(f64::from(-0.45_f32), 0.0, 0.0));
        assert_eq!(source.color, Color(0x0000ffff));
        let expected = if limit < 0 { 3 } else { limit.min(3) };
        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image.as_slice(), interpreted.render(&mut preview_gpu)?);
        if expected == 2 {
            assert_eq!(image.as_slice(), aot.render(&mut renderer()?)?);
        }
        let after = gpu.scene_draw_stats();
        assert_eq!(
            after.color_draws - before.color_draws,
            2 * (u64::from(expected != 0) + u64::from(expected == 3))
        );
        assert_eq!(
            after.color_instances - before.color_instances,
            2 * expected as u64
        );

        let model = app.get_snapshot();
        let hit = app.get_exact();
        assert_eq!(dynamic.get("exact")?, Value::Hit3D(hit.clone()));
        assert_eq!(hit.valid, expected != 0);
        let first = app.get_first();
        assert_eq!(first.valid, expected != 0);
        assert_eq!(dynamic.get("first")?, Value::Instance3D(first.clone()));
        assert!(!model.instance([1, -1, expected]).valid);
        let mirror_hit = app.get_mirror_hit();
        assert_eq!(dynamic.get("mirror-hit")?, Value::Hit3D(mirror_hit.clone()));
        assert_eq!(mirror_hit.valid, expected == 3);
        if expected > 0 {
            assert_eq!(
                (hit.row_index, hit.mesh_node_index, hit.instance_index),
                (1, -1, 0)
            );
            assert_eq!(first.origin, Point3D::new(-2.0 + offset, 1.0, 0.0));
            assert_eq!(first.color, Color(0xff0000ff));
            assert_eq!(first.data_w, 0.5);
            let second_row = model.instance([2, -1, 0]);
            assert_eq!(second_row.origin, Point3D::new(-2.0 + offset, -1.0, 0.0));
            assert_eq!(second_row.color, first.color);
            assert_eq!(second_row.data_w, first.data_w);
        }
        if expected == 3 {
            let rotated = model.instance([1, -1, 1]);
            assert_eq!(rotated.axis_x, Vector3D::new(f64::from(-0.4f32), 0.0, 0.0));
            assert_eq!(rotated.axis_y, Vector3D::new(0.0, f64::from(-0.6f32), 0.0));
            assert_eq!(rotated.color, Color(0x00ff00ff));
            let mirrored = model.instance([2, -1, 2]);
            assert!(mirrored.valid);
            assert_eq!(
                mirrored.axis_x,
                Vector3D::new(f64::from(-0.45f32), 0.0, 0.0)
            );
            assert_eq!(mirrored.color, Color(0x0000ffff));
            assert_eq!((mirror_hit.row_index, mirror_hit.instance_index), (2, 2));
            assert!(mirror_hit.normal.z > 0.99);
        }
        Ok((image, table, gpu.scene_mesh_uploads(), after))
    };

    let viewport = |png: &[u8]| -> TestResult<Vec<u8>> {
        let raster = xrack_assets::decode_png(png)?;
        Ok(raster.rgba[..282 * 4 * raster.size[0] as usize].to_vec())
    };
    let (_, _, positive_uploads, _) = frame(false, 1, 0.0)?;
    let (xml, table, uploads, _) = frame(false, -1, 0.0)?;
    assert_eq!(
        uploads, positive_uploads,
        "reflections share resident geometry"
    );
    let (binary, same_table, reused, resident) = frame(true, -1, 0.0)?;
    assert!(
        viewport(&xml)? == viewport(&binary)?,
        "XML and QtIR draw identical viewport pixels"
    );
    assert_eq!(table, same_table);
    assert_eq!(reused, uploads);
    // Neither viewport has prepared the two-row prefix yet. Both mesh runs
    // can draw it directly from their respective ranges in the full buffer.
    for count in [2, 0, 2, 3, 2, 1, 3] {
        let (_, _, _, current) = frame(true, count, 0.0)?;
        assert_eq!(
            current.instance_upload_bytes,
            resident.instance_upload_bytes
        );
        assert_eq!(
            current.instance_buffer_allocations,
            resident.instance_buffer_allocations
        );
    }
    let (trimmed, _, reused, _) = frame(true, 1, 0.75)?;
    assert_ne!(trimmed, binary);
    assert_eq!(reused, uploads);
    assert_ne!(frame(true, 0, 0.0)?.0, trimmed);
    assert!(
        viewport(&frame(true, 100, 0.0)?.0)? == viewport(&binary)?,
        "limit saturates at the table size"
    );
    Ok(())
}

#[test]
fn authored_instance_table_tracks_row_edits_and_reuses_gpu_streams() -> TestResult {
    use file_instances_ui::FileInstancesDemo;

    const NAME: &str = "examples/puzzle3d/file_instances.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("FileInstancesDemo")?;
    let mut app = FileInstancesDemo::new();
    app.set_authored(true);
    let weak = app.as_weak();
    dynamic.set("authored", Value::Bool(true))?;
    let mut aot = PreviewScene::new(Box::new(app));
    let mut app = weak.upgrade().ok_or("missing file instances demo")?;
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut check_frame = |gpu: &mut Renderer, preview_gpu: &mut Renderer| -> TestResult<Vec<u8>> {
        let image = aot.render(gpu)?.to_vec();
        assert_eq!(image.as_slice(), interpreted.render(preview_gpu)?);
        Ok(image)
    };
    let initial = check_frame(&mut gpu, &mut preview_gpu)?;
    let previous = app.get_authored_table();
    assert!(previous.is_loaded());
    assert_eq!(previous.count(), 3);
    let before_row = app.get_source_instance();
    assert_eq!(before_row.origin.y, 0.0);
    assert_eq!(
        dynamic.get("table")?,
        Value::InstanceTable3D(previous.clone())
    );
    let original_third = app.get_snapshot().instance([2, -1, 2]);
    assert!(original_third.valid && original_third.axis_x.x < 0.0);

    let before = gpu.scene_draw_stats();
    let mesh_uploads = gpu.scene_mesh_uploads();
    app.invoke_move_third(0.45, -0.45);
    dynamic.invoke("move-third", &[Value::Float(0.45), Value::Float(-0.45)])?;
    assert_ne!(initial, check_frame(&mut gpu, &mut preview_gpu)?);
    let updated = app.get_authored_table();
    assert_ne!(previous, updated);
    assert_eq!(
        dynamic.get("table")?,
        Value::InstanceTable3D(updated.clone())
    );
    let shifted = app.get_snapshot().instance([2, -1, 2]);
    let source = app.get_source_instance();
    assert_eq!(
        dynamic.get("source-instance")?,
        Value::Instance3D(source.clone())
    );
    assert_eq!(source, updated.instance(2));
    assert!((source.origin.y - 0.45).abs() < 1e-6);
    assert!((source.data_w - 0.45).abs() < 1e-6);
    assert_eq!(previous.instance(2), before_row);
    assert!(shifted.valid && shifted.axis_x.x < 0.0 && shifted.origin.y > original_third.origin.y);
    assert!((shifted.data_w - 0.45).abs() < 1e-6);
    assert_eq!(app.get_mirror_hit().instance_index, 2);
    assert_eq!(
        dynamic.get("mirror-hit")?,
        Value::Hit3D(app.get_mirror_hit())
    );
    let after = gpu.scene_draw_stats();
    assert_eq!(
        after.instance_buffer_allocations,
        before.instance_buffer_allocations
    );
    assert_eq!(
        after.instance_upload_bytes - before.instance_upload_bytes,
        168
    );
    assert!(after.partial_instance_uploads > before.partial_instance_uploads);
    assert_eq!(gpu.scene_mesh_uploads(), mesh_uploads);

    app.invoke_move_third(0.0, 0.0);
    dynamic.invoke("move-third", &[Value::Float(0.0), Value::Float(0.0)])?;
    let invalid = app.get_authored_table();
    assert!(!invalid.is_loaded());
    assert!(!app.get_source_instance().valid);
    assert_eq!(dynamic.get("table")?, Value::InstanceTable3D(invalid));

    app.invoke_clear_rows();
    dynamic.invoke("clear-rows", &[])?;
    check_frame(&mut gpu, &mut preview_gpu)?;
    let empty = app.get_authored_table();
    assert!(empty.is_loaded() && empty.is_empty());
    assert!(!app.get_source_instance().valid);
    assert!(app.get_snapshot().is_empty());
    assert!(previous.is_loaded() && previous.len() == 3);
    assert_eq!(dynamic.get("table")?, Value::InstanceTable3D(empty));
    Ok(())
}

#[test]
fn instance_transparency_keeps_material_coverage_and_reuses_geometry() -> TestResult {
    use instance_transparency_ui::InstanceTransparencyDemo;

    const NAME: &str = "examples/puzzle3d/instance_transparency.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let dynamic = xrack_interpreter::Program::from_module(&preview.module)?
        .create("InstanceTransparencyDemo")?;
    let generated = InstanceTransparencyDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut frame = |enabled, mode: &str, cutoff, weighted, depth_effects| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing instance transparency demo")?;
        app.set_instance_alpha(enabled);
        app.set_mode(mode.into());
        app.set_cutoff(cutoff);
        app.set_weighted(weighted);
        app.set_depth_effects(depth_effects);
        for (name, value) in [
            ("instance-alpha", Value::Bool(enabled)),
            ("mode", Value::String(mode.into())),
            ("cutoff", Value::Float(cutoff)),
            ("weighted", Value::Bool(weighted)),
            ("depth-effects", Value::Bool(depth_effects)),
        ] {
            dynamic.set(name, value)?;
        }
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image.as_slice(), interpreted.render(&mut preview_gpu)?);
        let hit = app.get_center_hit();
        assert_eq!(dynamic.get("center-hit")?, Value::Hit3D(hit.clone()));
        let rows = app.get_center_rows();
        let dynamic_rows = match dynamic.get("center-rows")? {
            Value::IntList(rows) => rows.to_vec(),
            Value::IntModel(rows) => rows.snapshot(),
            other => return Err(format!("unexpected selection: {other:?}").into()),
        };
        assert_eq!(rows, dynamic_rows);
        let raster = xrack_assets::decode_png(&image)?;
        assert_eq!(raster.size, [400, 326]);
        let pixels = [92, 200, 308].map(|x| {
            let start = (120 * 400 + x) * 4;
            std::array::from_fn(|channel| raster.rgba[start + channel])
        });
        Ok((
            pixels,
            hit,
            rows,
            gpu.scene_mesh_uploads(),
            gpu.scene_instance_uploads(),
            gpu.scene_draw_stats(),
        ))
    };
    let opaque = frame(false, "opaque", 0.2, false, false)?;
    assert_eq!(opaque.1.object_id, "panels");
    assert!(opaque.2.contains(&1));

    let blended = frame(true, "opaque", 0.2, false, false)?;
    assert_eq!(blended.1.object_id, "backdrop");
    assert_eq!(blended.2, [2]);
    assert_eq!(blended.0[1], [0x28, 0x50, 0xc0, 255]);
    assert!(
        blended.0[0][0] < opaque.0[0][0] && blended.0[0][2] > opaque.0[0][2],
        "expected a red/blue mix, got {:?} from {:?}",
        blended.0[0],
        opaque.0[0]
    );
    assert_eq!(blended.0[2], opaque.0[2]);
    assert_eq!((blended.3, blended.4), (opaque.3, opaque.4));

    let masked = frame(true, "mask", 0.2, false, false)?;
    assert_eq!(
        masked.0, blended.0,
        "instance alpha is separate from the mask cutoff"
    );
    let clipped = frame(true, "mask", 0.6, false, false)?;
    assert_eq!(clipped.0, [blended.0[1]; 3]);
    let explicit = frame(false, "blend", 0.2, false, false)?;
    assert_eq!(frame(true, "blend", 0.2, false, false)?.0, explicit.0);
    assert!(
        explicit.0[0][0] < blended.0[0][0],
        "the explicit blend still uses material alpha"
    );

    let weighted = frame(true, "opaque", 0.2, true, false)?;
    assert_eq!(weighted.0[1], blended.0[1]);
    assert!(weighted.0[0][0] < opaque.0[0][0] && weighted.0[0][2] > opaque.0[0][2]);
    assert!(weighted.5.transparency_composites > explicit.5.transparency_composites);
    let depth = frame(true, "mask", 0.2, false, true)?;
    assert!(depth.5.material_depth_prepass_draws > weighted.5.material_depth_prepass_draws);
    assert!(depth.5.motion_vector_draws > weighted.5.motion_vector_draws);
    assert_eq!(depth.3, opaque.3);
    Ok(())
}

#[test]
fn instance_depth_sorting_orders_each_table_as_one_scene_group() -> TestResult {
    use instance_transparency_ui::InstanceTransparencyDemo;

    const NAME: &str = "examples/puzzle3d/instance_transparency.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let dynamic = xrack_interpreter::Program::from_module(&preview.module)?
        .create("InstanceTransparencyDemo")?;
    let mut generated = InstanceTransparencyDemo::new();
    generated.set_stacked(true);
    generated.set_instance_alpha(true);
    dynamic.set("stacked", Value::Bool(true))?;
    dynamic.set("instance-alpha", Value::Bool(true))?;
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut frame = |sorted, spacing, bias, weighted| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing instance transparency demo")?;
        app.set_depth_sorting(sorted);
        app.set_spacing(spacing);
        app.set_table_bias(bias);
        app.set_weighted(weighted);
        for (name, value) in [
            ("depth-sorting", Value::Bool(sorted)),
            ("spacing", Value::Float(spacing)),
            ("table-bias", Value::Float(bias)),
            ("weighted", Value::Bool(weighted)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image.as_slice(), interpreted.render(&mut preview_gpu)?);
        let hit = app.get_center_hit();
        assert_eq!(dynamic.get("center-hit")?, Value::Hit3D(hit.clone()));
        let raster = xrack_assets::decode_png(&image)?;
        let start = (120 * 400 + 200) * 4;
        let [red, green, blue, _] = raster.rgba[start..start + 4].try_into()?;
        let after = gpu.scene_draw_stats();
        Ok((
            [red, green, blue],
            hit,
            after.color_draws - before.color_draws,
            after.instance_upload_bytes,
            gpu.scene_mesh_uploads(),
        ))
    };
    let ordered = frame(false, 0.7, 0.0, false)?;
    let sorted = frame(true, 0.7, 0.0, false)?;
    assert!(
        ordered.0[2] > ordered.0[1] && ordered.0[1] > ordered.0[0],
        "{:?}",
        ordered.0
    );
    assert!(
        sorted.0[2] > sorted.0[0] && sorted.0[0] > sorted.0[1],
        "{:?}",
        sorted.0
    );
    assert_eq!(
        sorted.0[2], ordered.0[2],
        "the blue marker stays in front of the whole table"
    );
    assert_eq!(ordered.2, 3, "the two adjacent rows share one draw");
    assert_eq!(sorted.2, 4, "the reversed rows draw in the required order");
    assert_eq!((ordered.3, ordered.4), (sorted.3, sorted.4));
    assert_eq!(sorted.1, ordered.1);
    assert_eq!(sorted.1.instance_index, 0);

    let biased = frame(true, 0.7, -2.0, false)?;
    assert!(
        biased.0[0] > biased.0[1] && biased.0[1] > biased.0[2],
        "{:?}",
        biased.0
    );
    assert_eq!((biased.3, biased.4), (sorted.3, sorted.4));
    assert_eq!(biased.1, sorted.1);
    assert_eq!(
        frame(false, 0.0, 0.0, false)?.0,
        frame(true, 0.0, 0.0, false)?.0
    );
    assert_eq!(
        frame(false, 0.7, 0.0, true)?.0,
        frame(true, 0.7, 0.0, true)?.0
    );
    Ok(())
}

#[test]
fn quaternion_orientations_match_scene_hierarchy_table_axes_and_picking() -> TestResult {
    use quaternion_scene_ui::QuaternionSceneDemo;
    use std::{
        f64::consts::PI,
        time::{Duration, Instant},
    };

    const NAME: &str = "examples/puzzle3d/quaternion_scene.xr";
    let source = std::fs::read_to_string(NAME)?;
    let document = xrack_compiler::parse(&source)
        .map_err(|diagnostics| format!("parse quaternion scene: {diagnostics:?}"))?;
    let raw_module = xrack_compiler::mlir::lower_module(&document, NAME)
        .map_err(|diagnostics| format!("lower quaternion scene: {diagnostics:?}"))?;
    let reference =
        xrack_interpreter::Program::from_module(&raw_module)?.create("QuaternionSceneDemo")?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let dynamic =
        xrack_interpreter::Program::from_module(&preview.module)?.create("QuaternionSceneDemo")?;
    let generated = QuaternionSceneDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut images = Vec::new();
    let near = |a: f64, b: f64| assert!((a - b).abs() < 1e-6, "{a} != {b}");
    let mut uploads = 0;
    weak.upgrade()
        .ok_or("missing quaternion demo")?
        .set_progress(0.0);
    dynamic.set("progress", Value::Float(0.0))?;
    for (progress, [x, y]) in [(0.0, [1.0, 0.0]), (0.5, [0.0, 1.0]), (1.0, [-1.0, 0.0])] {
        let app = weak.upgrade().ok_or("missing quaternion demo")?;
        near(app.get_progress(), progress);
        assert_eq!(dynamic.get("progress")?, Value::Float(progress));
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image.as_slice(), interpreted.render(&mut preview_gpu)?);
        if progress == 0.0 {
            uploads = gpu.scene_mesh_uploads();
        } else {
            assert_eq!(gpu.scene_mesh_uploads(), uploads);
        }
        images.push(image);
        let center = app.get_center();
        near(center.x, x + 0.75);
        near(center.y, y);
        assert_eq!(dynamic.get("center")?, Value::Point3D(center));
        let direction = app.get_direction();
        near(direction.x, x);
        near(direction.y, y);
        assert_eq!(dynamic.get("direction")?, Value::Vector3D(direction));
        assert_eq!(
            dynamic.get("rotation")?,
            Value::Quaternion3D(app.get_rotation())
        );

        let source = app.get_source_first();
        assert!(source.valid);
        near(source.axis_x.x, -x);
        near(source.axis_x.y, -y);
        assert_eq!(dynamic.get("source-first")?, Value::Instance3D(source));
        let hit = app.get_hit();
        assert!(hit.valid);
        assert_eq!(hit.object_id, "arm");
        assert_eq!(dynamic.get("hit")?, Value::Hit3D(hit));

        let rotate = Point { x: 72.0, y: 294.0 };
        aot.click(rotate);
        interpreted.click(rotate);
    }
    near(
        weak.upgrade()
            .ok_or("missing quaternion demo")?
            .get_progress(),
        0.0,
    );
    assert_eq!(dynamic.get("progress")?, Value::Float(0.0));
    assert!(images.windows(2).all(|pair| pair[0] != pair[1]));

    let play = Point { x: 185.0, y: 294.0 };
    aot.click(play);
    interpreted.click(play);
    let start = Instant::now();
    let mut previous = None;
    for (millis, seconds) in [
        (0, 0.0_f64),
        (250, 0.25),
        (500, 0.5),
        (1000, 1.0),
        (1500, 1.5),
        (2000, 2.0),
        (2250, 2.0),
    ] {
        let now = start + Duration::from_millis(millis);
        aot.advance_at(&mut gpu, now);
        interpreted.advance_at(&mut preview_gpu, now);

        let app = weak.upgrade().ok_or("missing quaternion demo")?;
        let track = app.get_track();
        assert!(track.valid());
        assert_eq!((track.count(), track.duration()), (3, 2.0));
        assert_eq!(dynamic.get("track")?, Value::RotationTrack3D(track));
        let camera_track = app.get_camera_track();
        assert_eq!((camera_track.count(), camera_track.duration()), (2, 2.0));
        assert_eq!(
            dynamic.get("camera-track")?,
            Value::RotationTrack3D(camera_track)
        );
        assert_eq!(dynamic.get("camera-track")?, reference.get("camera-track")?);
        reference.set("progress", Value::Float(seconds))?;
        assert_eq!(dynamic.get("track")?, reference.get("track")?);
        assert_eq!(dynamic.get("rotation")?, reference.get("rotation")?);
        assert_eq!(dynamic.get("hit")?, reference.get("hit")?);
        near(app.get_progress(), seconds);
        assert_eq!(dynamic.get("progress")?, Value::Float(seconds));
        assert_eq!(app.get_playing(), millis < 2000);
        assert_eq!(dynamic.get("playing")?, Value::Bool(app.get_playing()));
        let angle = (1.0 - (seconds - 1.0).abs()) * PI;
        let center = app.get_center();
        near(center.x, angle.cos() + 0.75);
        near(center.y, angle.sin());
        assert_eq!(dynamic.get("center")?, Value::Point3D(center));
        assert_eq!(app.get_hit().object_id, "arm");
        assert_eq!(dynamic.get("hit")?, Value::Hit3D(app.get_hit()));

        let frame = aot.draw_prepared(&mut gpu)?.to_vec();
        assert_eq!(&frame, interpreted.draw_prepared(&mut preview_gpu)?);
        assert_eq!(gpu.scene_mesh_uploads(), uploads);
        if let Some(previous) = previous {
            assert_eq!(frame == previous, millis > 2000);
        }
        previous = Some(frame);
    }
    assert!(aot.next_frame_delay().is_none());
    assert!(interpreted.next_frame_delay().is_none());

    let mut app = weak.upgrade().ok_or("missing quaternion demo")?;
    let first = app.get_track();
    let target = xrack_runtime::Quaternion3D::new(0.0, 0.0, 1.0, 1.0);
    app.set_target(target);
    dynamic.set("target", target.into())?;
    reference.set("target", target.into())?;
    assert_ne!(app.get_track(), first);
    assert_eq!(app.get_track().sample(1.0), target);
    assert_eq!(
        dynamic.get("track")?,
        Value::RotationTrack3D(app.get_track())
    );
    assert_eq!(dynamic.get("track")?, reference.get("track")?);
    Ok(())
}

#[test]
fn instance_shadow_envelopes_fit_once_and_preserve_offscreen_casters() -> TestResult {
    use instance_shadow_bounds_ui::InstanceShadowBoundsDemo;

    const NAME: &str = "examples/puzzle3d/instance_shadow_bounds.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let dynamic = xrack_interpreter::Program::from_module(&preview.module)?
        .create("InstanceShadowBoundsDemo")?;
    let generated = InstanceShadowBoundsDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    struct Frame {
        pixels: Vec<u8>,
        fits: u64,
        passes: u64,
        x: f64,
    }

    let mut render =
        |light: &str, cascades, shadows, bounded, envelope, shift| -> TestResult<Frame> {
            let mut app = weak.upgrade().ok_or("missing shadow bounds demo")?;
            app.set_light_kind(light.into());
            app.set_cascades(cascades);
            app.set_shadows(shadows);
            app.set_bounded(bounded);
            app.set_envelope(envelope);
            app.set_shift(shift);
            for (name, value) in [
                ("light-kind", Value::String(light.into())),
                ("cascades", Value::Int(cascades)),
                ("shadows", Value::Bool(shadows)),
                ("bounded", Value::Bool(bounded)),
                ("envelope", Value::Float(envelope)),
                ("shift", Value::Float(shift)),
            ] {
                dynamic.set(name, value)?;
            }
            let before = gpu.scene_draw_stats();
            let previous_shadows = gpu.scene_shadow_passes();
            let png = aot.render(&mut gpu)?.to_vec();
            assert_eq!(png.as_slice(), interpreted.render(&mut preview_gpu)?);
            let after = gpu.scene_draw_stats();
            assert_eq!(
                after.color_instances - before.color_instances,
                1,
                "the wall is the only visible mesh"
            );
            assert_eq!(app.get_picked(), "wall");
            assert_eq!(dynamic.get("picked")?, Value::String(app.get_picked()));
            let first = app.get_first();
            assert!(first.valid && first.instance_index == 0);
            assert_eq!(dynamic.get("first")?, Value::Instance3D(first.clone()));
            let raster = xrack_assets::decode_png(&png)?;
            assert_eq!(raster.size, [400, 286]);
            Ok(Frame {
                pixels: raster.rgba[..400 * 240 * 4].to_vec(),
                fits: after.shadow_fit_bounds - before.shadow_fit_bounds,
                passes: gpu.scene_shadow_passes() - previous_shadows,
                x: first.origin.x,
            })
        };
    let dark_pixels = |lit: &Frame, shadow: &Frame| {
        lit.pixels
            .chunks_exact(4)
            .zip(shadow.pixels.chunks_exact(4))
            .filter(|(lit, dark)| lit[0].saturating_sub(dark[0]) >= 8)
            .count()
    };
    for (light, cascades) in [
        ("directional", 1),
        ("directional", 3),
        ("point", 1),
        ("spot", 1),
    ] {
        let lit = render(light, cascades, false, false, 5.0, 0.0)?;
        assert_eq!((lit.fits, lit.passes), (0, 0));
        let automatic = render(light, cascades, true, false, 5.0, 0.0)?;
        let bounded = render(light, cascades, true, true, 5.0, 0.0)?;
        assert_eq!(
            (automatic.fits, bounded.fits),
            (129, 2),
            "{light} {cascades}"
        );
        assert!(automatic.passes > 0 && bounded.passes > 0);
        assert!(
            dark_pixels(&lit, &automatic) > 40,
            "automatic {light} {cascades}"
        );
        assert!(
            dark_pixels(&lit, &bounded) > 40,
            "bounded {light} {cascades}"
        );

        let warm = render(light, cascades, true, true, 5.0, 0.0)?;
        assert_eq!((warm.fits, warm.passes), (0, 0));
        assert_eq!(warm.pixels, bounded.pixels);
        let invalid = render(light, cascades, true, true, 2.0, 0.0)?;
        assert_eq!(invalid.pixels, automatic.pixels);
        let widened = render(light, cascades, true, true, 6.0, 0.0)?;
        assert_eq!(
            widened.fits, 2,
            "changing the envelope invalidates the shadow fit"
        );
        assert!(widened.passes > 0);
    }
    let origin = render("directional", 1, true, true, 5.0, 0.0)?;
    let shifted = render("directional", 1, true, true, 5.0, 0.4)?;
    assert!((shifted.x - origin.x - 0.4).abs() < 1e-5);
    assert_eq!(shifted.fits, 2);
    assert_ne!(shifted.pixels, origin.pixels);
    Ok(())
}

#[test]
fn random_instance_table_keeps_prefixes_and_shares_lod_picking_and_colliders() -> TestResult {
    use random_instances_ui::RandomInstancesDemo;

    const NAME: &str = "examples/puzzle3d/random_instances.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("RandomInstancesDemo")?;
    let generated = RandomInstancesDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut frame = |count: i64, seed, grid, coarse, offset, imported| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing random instance demo")?;
        app.set_count(count);
        app.set_seed(seed);
        app.set_grid(grid);
        app.set_coarse(coarse);
        app.set_imported(imported);
        app.set_offset(offset);
        for (name, value) in [
            ("count", Value::Int(count)),
            ("seed", Value::Int(seed)),
            ("grid", Value::Bool(grid)),
            ("coarse", Value::Bool(coarse)),
            ("imported", Value::Bool(imported)),
            ("offset", Value::Float(offset)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image.as_slice(), interpreted.render(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        let table_count = count.clamp(0, if grid { 8 } else { 4096 }) as u64;
        let table = app.get_table();
        assert!(table.is_loaded());
        assert_eq!(table.count(), table_count as i64);
        assert_eq!(dynamic.get("table")?, Value::InstanceTable3D(table.clone()));
        let source_first = table.instance(0);
        assert_eq!(app.get_source_first(), source_first);
        assert_eq!(
            dynamic.get("source-first")?,
            Value::Instance3D(source_first.clone())
        );
        assert_eq!(source_first.valid, table_count != 0);
        assert!(!table.instance(table_count as i64).valid);
        let visible = table_count.min(u64::try_from(app.get_draw_limit()).unwrap_or(u64::MAX));
        let echo = u64::from(app.get_reuse_table() && table_count != 0);
        let draws_per_table = if imported { 2 } else { 1 };
        let shares_box_draw = coarse && !imported && visible != 0 && echo != 0;
        assert_eq!(
            after.color_draws - before.color_draws,
            draws_per_table * u64::from(visible != 0) + echo - u64::from(shares_box_draw)
        );
        let parts_per_instance = if imported { 3 } else { 1 };
        assert_eq!(
            after.color_instances - before.color_instances,
            visible * parts_per_instance + echo
        );
        let vertices = if imported {
            9
        } else if coarse {
            36
        } else {
            Mesh3D::Sphere.vertices().len() as u64
        };
        assert_eq!(
            after.color_vertices - before.color_vertices,
            visible * vertices + echo * 36
        );

        let model = app.get_snapshot();
        let node_index = if imported { 0 } else { -1 };
        let step = if imported { 4096 } else { 1 };
        let positions: Vec<_> = (0..visible)
            .map(|index| model.instance([1, node_index, index as i64 * step]))
            .collect();
        assert!(positions.iter().all(|value| value.valid));
        if !imported && let Some(first) = positions.first() {
            assert!((first.origin.x - source_first.origin.x - offset).abs() < 1e-5);
            assert_eq!(first.origin.y, source_first.origin.y);
            assert_eq!(first.axis_x, source_first.axis_x);
            assert_eq!(first.color, source_first.color);
            assert_eq!(first.data_w, source_first.data_w);
        }
        let echo_row = model.instance([4, -1, 0]);
        assert_eq!(echo_row.valid, echo != 0);
        assert!(!model.instance([4, -1, 1]).valid);
        if echo_row.valid {
            assert!((echo_row.origin.y - source_first.origin.y + 1.2).abs() < 1e-5);
            assert_eq!(echo_row.data_w, source_first.data_w);
        }
        if grid {
            let cells = positions
                .iter()
                .map(|value| (value.origin.x.to_bits(), value.origin.y.to_bits()))
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(cells.len(), positions.len());
        }
        assert!(!model.instance([1, node_index, visible as i64 * step]).valid);
        if imported {
            for index in 0..visible as i64 {
                let nested = model.instance([1, 0, index * step + 1]);
                let accent = model.instance([1, 1, index]);
                assert!(nested.valid && accent.valid);
                assert_eq!(nested.color, accent.color);
                assert_eq!(nested.data_w, accent.data_w);
            }
        }
        assert_eq!(dynamic.get("first")?, Value::Instance3D(app.get_first()));
        let exact = app.get_exact();
        assert_eq!(exact.valid, visible != 0);
        assert_eq!(dynamic.get("exact")?, Value::Hit3D(exact.clone()));
        let overlap = app.get_overlap();
        let Value::Hit3DList(dynamic_overlap) = dynamic.get("overlap")? else {
            return Err("missing interpreted collider hits".into());
        };
        assert_eq!(overlap, dynamic_overlap.snapshot());
        if visible != 0 {
            assert_eq!(
                (exact.row_index, exact.mesh_node_index, exact.instance_index),
                (1, node_index, 0)
            );
            assert!(
                overlap
                    .iter()
                    .any(|hit| hit.row_index == 1 && hit.instance_index == 0)
            );
        } else {
            assert!(overlap.is_empty());
        }
        Ok((image, positions, gpu.scene_mesh_uploads()))
    };

    let (initial, prefix, uploads) = frame(6, 7, true, false, 0.0, false)?;
    let (grown, full, reused) = frame(12, 7, true, false, 0.0, false)?;
    assert_eq!(reused, uploads);
    assert_ne!(initial, grown);
    assert_eq!(full[..prefix.len()], prefix);
    let (coarse_image, coarse_positions, _) = frame(12, 7, true, true, 0.0, false)?;
    assert_ne!(grown, coarse_image);
    assert_eq!(full, coarse_positions);
    let (_, translated, _) = frame(12, 7, true, true, 0.75, false)?;
    for (old, new) in full.iter().zip(translated) {
        assert_eq!(new.origin.x, old.origin.x + 0.75);
        assert_eq!(new.data_w, old.data_w);
        assert_eq!(new.color, old.color);
    }
    let (_, reseeded, _) = frame(12, 8, true, true, 0.0, false)?;
    assert_ne!(full, reseeded);
    assert_eq!(frame(12, 7, false, true, 0.0, false)?.1.len(), 12);
    assert!(frame(0, 7, true, false, 0.0, false)?.1.is_empty());
    let (_, imported, imported_uploads) = frame(6, 7, true, false, 0.0, true)?;
    let (_, imported_more, reused) = frame(8, 7, true, false, 0.0, true)?;
    assert_eq!(imported_more[..imported.len()], imported);
    assert_eq!(imported_uploads, reused);

    let viewport = |png: &[u8]| -> TestResult<Vec<u8>> {
        Ok(xrack_assets::decode_png(png)?.rgba[..640 * 278 * 4].to_vec())
    };
    let mut app = weak.upgrade().ok_or("missing random instance demo")?;
    let (shared, shared_rows, _) = frame(12, 7, true, true, 0.0, false)?;
    app.set_shared_table(false);
    dynamic.set("shared-table", Value::Bool(false))?;
    let (legacy, legacy_rows, _) = frame(12, 7, true, true, 0.0, false)?;
    assert_eq!(shared_rows, legacy_rows);
    assert_eq!(viewport(&shared)?, viewport(&legacy)?);

    app.set_shared_table(true);
    app.set_reuse_table(true);
    app.set_draw_limit(2);
    dynamic.set("shared-table", Value::Bool(true))?;
    dynamic.set("reuse-table", Value::Bool(true))?;
    dynamic.set("draw-limit", Value::Int(2))?;
    assert_eq!(frame(12, 7, true, true, 0.0, false)?.1, shared_rows[..2]);
    app.set_draw_limit(0);
    dynamic.set("draw-limit", Value::Int(0))?;
    assert!(frame(12, 7, true, true, 0.0, false)?.1.is_empty());

    app.set_draw_limit(-1);
    app.set_reuse_table(false);
    dynamic.set("draw-limit", Value::Int(-1))?;
    dynamic.set("reuse-table", Value::Bool(false))?;
    let (rgb, rgb_rows, mesh_uploads) = frame(12, 7, true, true, 0.0, false)?;
    let mut previous = viewport(&rgb)?;
    for mode in ["hsv", "hsl"] {
        app.set_color_model(mode.into());
        dynamic.set("color-model", Value::String(mode.into()))?;
        let (image, rows, reused) = frame(12, 7, true, true, 0.0, false)?;
        assert_eq!(reused, mesh_uploads);
        let current = viewport(&image)?;
        assert_ne!(current, previous, "{mode}");
        previous = current;
        for (old, new) in rgb_rows.iter().zip(rows) {
            assert_eq!(old.origin, new.origin);
            assert_eq!(old.axis_x, new.axis_x);
            assert_eq!(old.data_w, new.data_w);
        }
    }
    Ok(())
}

#[test]
fn custom_instance_data_shares_draws_and_reprojects_vertex_changes() -> TestResult {
    use instance_data_ui::InstanceDataDemo;

    const NAME: &str = "examples/puzzle3d/instance_data.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("InstanceDataDemo")?;
    let generated = InstanceDataDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    struct Work {
        draws: u64,
        instances: u64,
        culled: u64,
        upload_bytes: u64,
        geometry_uploads: [u64; 2],
        shadows: u64,
        motion: u64,
        reprojects: u64,
    }
    let mut frame = |phase, shadow, temporal| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing instance data demo")?;
        app.set_phase(phase);
        app.set_shadow(shadow);
        app.set_temporal(temporal);
        for (name, value) in [
            ("phase", Value::Float(phase)),
            ("shadow", Value::Bool(shadow)),
            ("temporal", Value::Bool(temporal)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let shadows = gpu.scene_shadow_passes();
        let initial_uploads = [gpu.scene_mesh_uploads(), gpu.scene_instance_uploads()];
        let image = aot.render_animated(&mut gpu)?.to_vec();
        assert_eq!(image, interpreted.render_animated(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        Ok((
            xrack_assets::decode_png(&image)?,
            Work {
                draws: after.color_draws - before.color_draws,
                instances: after.color_instances - before.color_instances,
                culled: after.culled_instances - before.culled_instances,
                upload_bytes: after.instance_upload_bytes - before.instance_upload_bytes,
                geometry_uploads: [
                    gpu.scene_mesh_uploads() - initial_uploads[0],
                    gpu.scene_instance_uploads() - initial_uploads[1],
                ],
                shadows: gpu.scene_shadow_passes() - shadows,
                motion: after.motion_vector_draws - before.motion_vector_draws,
                reprojects: after.temporal_reprojections - before.temporal_reprojections,
            },
        ))
    };
    let pixel = |image: &xrack_assets::Raster, x, y| -> [u8; 3] {
        let offset = 4 * (y * image.size[0] as usize + x);
        std::array::from_fn(|channel| image.rgba[offset + channel])
    };
    let (start, base) = frame(0.0, false, false)?;
    assert_eq!([base.draws, base.instances, base.culled], [1, 3, 1]);
    let colors = [
        pixel(&start, 150, 150),
        pixel(&start, 300, 150),
        pixel(&start, 450, 150),
    ];
    assert_eq!(colors, [[255, 0, 0], [0, 255, 0], [0, 0, 255]]);

    let (changed, moved) = frame(1.0, false, false)?;
    assert_eq!([moved.draws, moved.instances, moved.culled], [1, 3, 1]);
    assert_eq!(moved.geometry_uploads, [0; 2]);
    assert_eq!(
        moved.upload_bytes, 32,
        "only one raw and one compacted data row"
    );
    assert_eq!(pixel(&changed, 350, 150), [255, 255, 0]);
    assert_eq!(pixel(&changed, 300, 150), pixel(&changed, 25, 25));
    assert_eq!(pixel(&changed, 150, 150), colors[0]);
    assert_eq!(pixel(&changed, 450, 150), colors[2]);

    assert!(frame(1.0, true, false)?.1.shadows > 0);
    let (unmoved, shadows) = frame(0.0, true, false)?;
    assert!(
        shadows.shadows > 0,
        "per-instance displacement invalidates cached shadows"
    );
    assert_eq!(shadows.geometry_uploads, [0; 2]);
    assert_eq!(pixel(&unmoved, 300, 150), colors[1]);

    assert!(frame(0.0, true, true)?.1.motion > 0);
    let (_, motion) = frame(1.0, true, true)?;
    assert!(motion.motion > 0 && motion.reprojects > 0);
    Ok(())
}

#[test]
fn instance_root_composes_model_local_offsets_after_each_placement() -> TestResult {
    use instance_root_ui::InstanceRootDemo;
    use xrack_runtime::{Hit3D, Length, Point2D, Point3D, Vector3D, scene3d::ray_hit_model};

    const NAME: &str = "examples/puzzle3d/instance_root.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("InstanceRootDemo")?;
    let generated = InstanceRootDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        root: &'static str,
        copy_root: &'static str,
        visible: bool,
        turn: bool,
        shift: f64,
    }
    let base = State {
        root: "anchor",
        copy_root: "",
        visible: true,
        turn: false,
        shift: 0.0,
    };
    let mut frame = |state: State, expected: Option<([[f64; 2]; 2], f64)>| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing instance root demo")?;
        let points = expected.map_or([[0.0, 0.0]; 2], |(points, _)| points);
        let viewport =
            |[x, y]: [f64; 2]| Point2D::new(Length(300.0 + 40.0 * x), Length(160.0 - 40.0 * y));
        app.set_root_name(state.root.into());
        app.set_copy_root(state.copy_root.into());
        app.set_anchor_visible(state.visible);
        app.set_shift(state.shift);
        app.set_turn(state.turn);
        app.set_box_pointer(viewport(points[0]));
        app.set_mesh_pointer(viewport(points[1]));
        for (key, value) in [
            ("root-name", Value::String(state.root.into())),
            ("copy-root", Value::String(state.copy_root.into())),
            ("anchor-visible", Value::Bool(state.visible)),
            ("shift", Value::Float(state.shift)),
            ("turn", Value::Bool(state.turn)),
            ("box-pointer", Value::Point2D(viewport(points[0]))),
            ("mesh-pointer", Value::Point2D(viewport(points[1]))),
        ] {
            dynamic.set(key, value)?;
        }

        let hits = [app.get_box_hit(), app.get_mesh_hit()];
        for (name, hit) in ["box-hit", "mesh-hit"].into_iter().zip(&hits) {
            assert_eq!(dynamic.get(name)?, Value::Hit3D(hit.clone()));
        }
        let frame = app.get_frame_origin();
        assert_eq!(dynamic.get("frame-origin")?, Value::Point3D(frame));
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image, interpreted.render(&mut preview_gpu)?);
        let (view, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(model)) = view.properties.get("model") else {
            return Err("missing instance root model".into());
        };

        if let Some((points, frame_x)) = expected {
            assert_eq!(frame, Point3D::new(frame_x, 0.0, 0.0));
            let ids = [("copy", 3, -1), ("group", 4, 1)];
            let key = |hit: &Hit3D| (hit.object_id.clone(), hit.row_index, hit.instance_index);
            let camera = model
                .camera("view", [600.0, 320.0])
                .ok_or("missing view camera")?;
            let selected = model.geometry(&camera);
            assert_eq!(selected.lod_objects, 1);
            let mut submitted = selected.iter().filter(|object| {
                object.id == "copy" || object.imported_placement() == Some((0, Some(1)))
            });
            for (([x, y], (id, row, instance)), hit) in points.into_iter().zip(ids).zip(&hits) {
                let placed = submitted.next().ok_or("missing transformed placement")?;
                let [render_x, render_y, _] = placed.transform_rows().map(|row| row[3]);
                assert!((f64::from(render_x) - x).abs() < 1e-4);
                assert!((f64::from(render_y) - y).abs() < 1e-4);
                assert!(hit.valid);
                assert_eq!(key(hit), (id.into(), row, instance));
                let physical = ray_hit_model(
                    model,
                    Point3D::new(x, y, 12.0),
                    Vector3D::new(0.0, 0.0, -1.0),
                    20.0,
                    None,
                );
                assert_eq!(key(&physical), key(hit));
            }
        } else {
            assert!(hits.iter().all(|hit| !hit.valid));
            assert!(model.is_empty());
        }

        Ok((image, gpu.scene_mesh_uploads()))
    };
    let (original, cached) = frame(base, Some(([[-1.0, 0.0], [4.0, -2.0]], -2.0)))?;
    let (turned, uploads) = frame(
        State { turn: true, ..base },
        Some(([[-2.0, 1.0], [4.0, -2.0]], -2.0)),
    )?;
    assert_ne!(turned, original);
    assert_eq!(uploads, cached);
    let (shifted, uploads) = frame(
        State {
            turn: true,
            shift: 1.0,
            ..base
        },
        Some(([[-1.0, 1.0], [5.0, -2.0]], -1.0)),
    )?;
    assert_ne!(shifted, turned);
    assert_eq!(uploads, cached);
    frame(
        State {
            turn: true,
            copy_root: "alternate",
            ..base
        },
        Some(([[0.0, -1.0], [4.0, -2.0]], 0.0)),
    )?;
    frame(
        State { root: "", ..base },
        Some(([[-3.0, 0.0], [1.0, -1.1]], -3.0)),
    )?;
    for state in [
        State {
            root: "missing",
            ..base
        },
        State {
            root: "copy",
            ..base
        },
        State {
            visible: false,
            ..base
        },
    ] {
        frame(state, None)?;
    }
    Ok(())
}

#[test]
fn camera_distance_filters_authored_instances_without_rebuilding_gpu_geometry() -> TestResult {
    use instance_table_ui::InstanceTableDemo;
    use xrack_runtime::{Point3D, Vector3D, scene3d::ray_hit_model};

    const NAME: &str = "examples/puzzle3d/instance_table.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("InstanceTableDemo")?;
    let mut generated = InstanceTableDemo::new();
    generated.set_distance_lod(true);
    dynamic.set("distance-lod", Value::Bool(true))?;
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut frame = |distance, culling, row, stats| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing instance table")?;
        app.set_eye_distance(distance);
        app.set_culling(culling);
        dynamic.set("eye-distance", Value::Float(distance))?;
        dynamic.set("culling", Value::Bool(culling))?;

        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        let after = gpu.scene_draw_stats();
        assert_eq!(image, interpreted.render(&mut preview_gpu)?);
        assert_eq!(
            [
                after.color_instances - before.color_instances,
                after.distance_filtered_instances - before.distance_filtered_instances,
                after.culled_instances - before.culled_instances,
            ],
            stats,
        );
        let hits = [app.get_left_hit(), app.get_right_hit()];
        for (name, hit) in ["left-hit", "right-hit"].into_iter().zip(&hits) {
            assert_eq!(dynamic.get(name)?, Value::Hit3D(hit.clone()));
        }
        assert_eq!(
            hits.map(|hit| hit.row_index),
            if row == 4 { [4, -1] } else { [-1, 5] }
        );
        let selected = match dynamic.get("selected-rows")? {
            Value::IntList(rows) => rows,
            Value::IntModel(model) => model.snapshot(),
            _ => return Err("missing distance-filtered selection".into()),
        };
        assert_eq!(selected, [row]);
        assert_eq!(app.get_selected_rows(), selected);

        let (node, _) = inspect(&aot)?;
        let Some(Value::SceneObjects(model)) = node.properties.get("model") else {
            return Err("missing instance table model".into());
        };
        let ray = ray_hit_model(
            model,
            Point3D::new(-1.3, 0.0, 8.0),
            Vector3D::new(0.0, 0.0, -1.0),
            16.0,
            None,
        );
        assert_eq!((&*ray.object_id, ray.row_index), ("left", 4));
        Ok((
            image,
            [gpu.scene_mesh_uploads(), gpu.scene_instance_uploads()],
        ))
    };
    let (near, uploads) = frame(8.0, true, 4, [1, 1, 1])?;
    let (far, far_uploads) = frame(12.0, true, 5, [1, 1, 1])?;
    assert_ne!(far, near);
    assert_eq!(uploads, far_uploads);
    let (far_without_frustum, far_uploads) = frame(12.0, false, 5, [2, 1, 0])?;
    assert_eq!(far, far_without_frustum);
    assert_eq!(uploads, far_uploads);
    assert_eq!(frame(8.0, true, 4, [1, 1, 1])?, (near, uploads));
    Ok(())
}

#[test]
fn picking_preserves_and_requeries_distinct_gltf_instance_addresses() -> TestResult {
    use instance_picking_ui::InstancePickingDemo;
    use xrack_runtime::{Hit3D, Point2D};

    const NAME: &str = "examples/puzzle3d/instance_picking.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("InstancePickingDemo")?;
    let generated = InstancePickingDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    #[derive(Clone, Copy)]
    struct State {
        address: [i64; 3],
        mask: i64,
        near: f64,
        distance_limit: f64,
        covered: bool,
        pickable: bool,
        x: f64,
    }
    let default = State {
        address: [1, 2, 1],
        mask: 3,
        near: 0.1,
        distance_limit: -1.0,
        covered: false,
        pickable: true,
        x: 320.0,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing instance picking demo")?;
        let [row, node, instance] = state.address;
        let pointer = Point2D::new(xrack_runtime::Length(state.x), xrack_runtime::Length(160.0));
        app.set_target_row(row);
        app.set_target_node(node);
        app.set_target_instance(instance);
        app.set_mask(state.mask);
        app.set_near(state.near);
        app.set_distance_limit(state.distance_limit);
        app.set_covered(state.covered);
        app.set_pickable(state.pickable);
        app.set_pointer(pointer);
        for (key, value) in [
            ("target-row", Value::Int(row)),
            ("target-node", Value::Int(node)),
            ("target-instance", Value::Int(instance)),
            ("mask", Value::Int(state.mask)),
            ("near", Value::Float(state.near)),
            ("distance-limit", Value::Float(state.distance_limit)),
            ("covered", Value::Bool(state.covered)),
            ("pickable", Value::Bool(state.pickable)),
            ("pointer", Value::Point2D(pointer)),
        ] {
            dynamic.set(key, value)?;
        }
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png, interpreted.render(&mut preview_gpu)?);
        let lists = [app.get_hits(), app.get_ray_hits(), app.get_selected_hits()];
        for (name, list) in ["hits", "ray-hits", "selected-hits"]
            .into_iter()
            .zip(&lists)
        {
            let Value::Hit3DList(other) = dynamic.get(name)? else {
                return Err(format!("missing interpreted list {name}").into());
            };
            assert_eq!(list, &other.snapshot());
        }
        let exact = [
            app.get_exact(),
            app.get_world_exact(),
            app.get_orbit_exact(),
        ];
        for (name, hit) in ["exact", "world-exact", "orbit-exact"]
            .into_iter()
            .zip(&exact)
        {
            assert_eq!(dynamic.get(name)?, Value::Hit3D(hit.clone()));
        }
        let nearest = app.get_nearest();
        assert_eq!(lists[0].first().cloned().unwrap_or_default(), nearest);
        assert_eq!(dynamic.get("nearest")?, Value::Hit3D(nearest));
        Ok((lists, exact))
    };
    let address = |hit: &Hit3D| [hit.row_index, hit.mesh_node_index, hit.instance_index];
    let addresses = |hits: &[Hit3D]| hits.iter().map(address).collect::<Vec<_>>();
    let expected = [[1, 2, 0], [1, 2, 1], [1, 0, 0], [1, 4, -1]];
    let (hits, exact) = render(default)?;
    for list in &hits {
        assert_eq!(addresses(list), expected);
        assert!(
            list.windows(2)
                .all(|pair| pair[0].distance < pair[1].distance)
        );
    }
    assert_eq!(exact[0], hits[0][1]);
    assert_eq!(exact[1], hits[1][1]);
    assert!(
        exact
            .iter()
            .all(|hit| hit.valid && address(hit) == default.address)
    );

    let (hits, exact) = render(State {
        distance_limit: 8.0,
        ..default
    })?;
    assert_eq!(addresses(&hits[0]), expected[..1]);
    assert_eq!(addresses(&hits[2]), expected[..1]);
    assert_eq!(addresses(&hits[1]), expected);
    assert!(!exact[0].valid && exact[1].valid && !exact[2].valid);
    let (hits, _) = render(State {
        distance_limit: 8.0,
        address: [1, 2, 2],
        x: 528.0,
        ..default
    })?;
    assert!(hits[0].is_empty(), "distance includes the lateral offset");

    let rear = [1, 0, 0];
    let (hits, exact) = render(State {
        address: rear,
        covered: true,
        ..default
    })?;
    assert_eq!(hits[0][0].object_id, "cover");
    assert_eq!(addresses(&hits[0][1..]), expected);
    assert_eq!(addresses(&hits[2]), expected);
    assert!(exact.iter().all(|hit| hit.valid && address(hit) == rear));

    let (hits, exact) = render(State {
        address: [1, 2, 0],
        near: 7.5,
        ..default
    })?;
    assert_eq!(addresses(&hits[0]), expected[1..]);
    assert_eq!(addresses(&hits[2]), expected[1..]);
    assert_eq!(addresses(&hits[1]), expected);
    assert!(!exact[0].valid);
    assert!(exact[1].valid && exact[2].valid);

    let (side, exact) = render(State {
        address: [1, 2, 2],
        x: 528.0,
        ..default
    })?;
    assert_eq!(addresses(&side[0]), [[1, 2, 2]]);
    assert_eq!(address(&exact[0]), [1, 2, 2]);
    assert!(
        !exact[1].valid,
        "the centered world ray misses the side instance"
    );
    assert!(render(State { x: -1.0, ..default })?.0[0].is_empty());

    for [row, node, index] in [
        [1, -1, 0],
        [1, 4, 0],
        [1, 2, 1i64 << 32],
        [0, 2, 0],
        [1, 2, 2],
    ] {
        let (_, misses) = render(State {
            address: [row, node, index],
            ..default
        })?;
        assert!(misses.iter().all(|hit| !hit.valid));
    }
    for state in [
        State { mask: 0, ..default },
        State { mask: 1, ..default },
        State {
            pickable: false,
            ..default
        },
    ] {
        let (hits, exact) = render(state)?;
        assert!(hits.iter().all(Vec::is_empty));
        assert!(exact.iter().all(|hit| !hit.valid));
    }
    Ok(())
}

#[test]
fn material_blend_modes_match_linear_compositing_in_both_backends() -> TestResult {
    use material_blend_ui::MaterialBlendDemo;
    use xrack_runtime::Color;

    const NAME: &str = "examples/puzzle3d/material_blend.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialBlendDemo")?;
    let generated = MaterialBlendDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        mode: &'static str,
        coverage: &'static str,
        tint: Color,
        weighted: bool,
        underlay: bool,
        offscreen: bool,
        smooth: bool,
        particles: bool,
    }
    let default = State {
        mode: "source-over",
        coverage: "blend",
        tint: Color(0xcc682880),
        weighted: false,
        underlay: false,
        offscreen: false,
        smooth: false,
        particles: false,
    };
    let mut render = |state: State| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing material blend demo")?;
        app.set_mode(state.mode.into());
        app.set_coverage(state.coverage.into());
        app.set_tint(state.tint);
        app.set_weighted(state.weighted);
        app.set_underlay(state.underlay);
        app.set_offscreen(state.offscreen);
        app.set_smooth(state.smooth);
        app.set_particles(state.particles);
        for (key, value) in [
            ("mode", Value::String(state.mode.into())),
            ("coverage", Value::String(state.coverage.into())),
            ("tint", Value::Color(state.tint)),
            ("weighted", Value::Bool(state.weighted)),
            ("underlay", Value::Bool(state.underlay)),
            ("offscreen", Value::Bool(state.offscreen)),
            ("smooth", Value::Bool(state.smooth)),
            ("particles", Value::Bool(state.particles)),
        ] {
            dynamic.set(key, value)?;
        }
        let before = gpu.scene_draw_stats();
        let image = aot.render(&mut gpu)?.to_vec();
        assert_eq!(image, interpreted.render(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        let picked = app.get_picked();
        assert_eq!(dynamic.get("picked")?, Value::String(picked.clone()));
        let raster = xrack_assets::decode_png(&image)?;
        assert_eq!(raster.size, [600, 352]);
        let offset = 4 * (150 * 600 + 300);
        let pixel = std::array::from_fn(|channel| raster.rgba[offset + channel]);
        Ok((
            pixel,
            picked,
            [
                after.transparency_composites - before.transparency_composites,
                after.color_draws - before.color_draws,
                gpu.scene_mesh_uploads(),
            ],
        ))
    };
    let linear = |byte: u8| {
        let s = f64::from(byte) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let srgb = |l: f64| {
        let s = if l <= 0.0031308 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    let expected = |mode, alpha, underlay| {
        let back = [0x40, 0x60, 0x90].map(linear);
        let mid = [0x34, 0x98, 0xe8].map(linear);
        let front = [0xcc, 0x68, 0x28].map(linear);
        let mid_alpha = if underlay { 128.0 / 255.0 } else { 0.0 };
        std::array::from_fn::<_, 3, _>(|channel| {
            let dest = back[channel] + mid_alpha * (mid[channel] - back[channel]);
            let source = front[channel];
            let result = match mode {
                "screen" => source + dest * (1.0 - source),
                "multiply" => source * dest,
                _ => source,
            };
            srgb(dest + alpha * (result - dest))
        })
    };
    let assert_color = |actual: [u8; 4], target: [u8; 3]| {
        assert_eq!(actual[3], 255);
        assert!(
            actual[..3]
                .iter()
                .zip(target)
                .all(|(&a, b)| a.abs_diff(b) <= 2),
            "{actual:?} != {target:?}"
        );
    };
    let (_, _, base_stats) = render(default)?;
    for mode in ["source-over", "screen", "multiply"] {
        let (pixel, picked, stats) = render(State { mode, ..default })?;
        assert_color(pixel, expected(mode, 128.0 / 255.0, false));
        assert_eq!(picked, "overlay");
        assert_eq!(stats[2], base_stats[2], "color compositing reuses geometry");
    }

    for mode in ["screen", "multiply"] {
        let state = State {
            mode,
            underlay: true,
            ..default
        };
        assert_color(render(state)?.0, expected(mode, 128.0 / 255.0, true));
        let (pixel, _, stats) = render(State {
            weighted: true,
            smooth: true,
            ..state
        })?;
        assert_color(pixel, expected(mode, 128.0 / 255.0, true));
        assert_eq!(
            stats[0], 0,
            "visible custom blend needs sorted mesh composition"
        );
        assert_eq!(stats[1], 3);
        let (pixel, _, stats) = render(State {
            weighted: true,
            particles: true,
            ..state
        })?;
        assert_color(pixel, expected(mode, 128.0 / 255.0, true));
        assert_eq!(
            stats[0], 1,
            "soft model particles retain their weighted pass"
        );
    }

    let (pixel, picked, stats) = render(State {
        mode: "multiply",
        weighted: true,
        underlay: true,
        offscreen: true,
        ..default
    })?;
    assert_color(pixel, expected("multiply", 0.0, true));
    assert_eq!(picked, "underlay");
    assert_eq!(
        stats[0], 1,
        "culled material does not disable weighted mesh composition"
    );
    let (pixel, picked, _) = render(State {
        mode: "screen",
        coverage: "mask",
        tint: Color(0xcc682840),
        ..default
    })?;
    assert_color(pixel, [0x40, 0x60, 0x90]);
    assert_eq!(picked, "background");
    let (pixel, picked, _) = render(State {
        mode: "screen",
        coverage: "mask",
        tint: Color(0xcc6828c0),
        ..default
    })?;
    assert_color(pixel, expected("screen", 1.0, false));
    assert_eq!(picked, "overlay");
    Ok(())
}

#[test]
fn material_depth_modes_preserve_coverage_and_transmission_in_both_backends() -> TestResult {
    use material_depth_ui::MaterialDepthDemo;

    const NAME: &str = "examples/puzzle3d/material_depth.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialDepthDemo")?;
    let generated = MaterialDepthDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    #[derive(Clone, Copy)]
    struct State {
        mode: &'static str,
        blended: bool,
        masked: bool,
        smooth: bool,
        weighted: bool,
        shadows: bool,
        glass: bool,
    }
    let initial = State {
        mode: "opaque-only",
        blended: true,
        masked: false,
        smooth: false,
        weighted: false,
        shadows: false,
        glass: false,
    };
    struct Frame {
        pixels: [[u8; 4]; 3],
        hits: [String; 3],
        prepass: u64,
        oit: u64,
        captures: u64,
        uploads: u64,
        shadows: u64,
        shadow_region: Vec<u8>,
    }
    let mut render = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing material depth demo")?;
        app.set_mode(state.mode.into());
        app.set_blended(state.blended);
        app.set_masked(state.masked);
        app.set_smooth(state.smooth);
        app.set_weighted(state.weighted);
        app.set_shadows(state.shadows);
        app.set_glass(state.glass);
        for (name, value) in [
            ("mode", Value::String(state.mode.into())),
            ("blended", Value::Bool(state.blended)),
            ("masked", Value::Bool(state.masked)),
            ("smooth", Value::Bool(state.smooth)),
            ("weighted", Value::Bool(state.weighted)),
            ("shadows", Value::Bool(state.shadows)),
            ("glass", Value::Bool(state.glass)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png, interpreted.render(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        let hits = [
            app.get_upper_hit(),
            app.get_center_hit(),
            app.get_lower_hit(),
        ];
        for (name, hit) in ["upper-hit", "center-hit", "lower-hit"]
            .into_iter()
            .zip(&hits)
        {
            assert_eq!(dynamic.get(name)?, Value::String(hit.clone()));
        }
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        let pixels = [88, 160, 232].map(|y| {
            let start = (y * 600 + 220) * 4;
            std::array::from_fn(|channel| raster.rgba[start + channel])
        });
        Ok(Frame {
            hits,
            pixels,
            prepass: after.material_depth_prepass_draws - before.material_depth_prepass_draws,
            oit: after.transparency_composites - before.transparency_composites,
            captures: after.transmission_captures - before.transmission_captures,
            uploads: gpu.scene_mesh_uploads(),
            shadows: gpu.scene_shadow_passes(),
            shadow_region: raster
                .rgba
                .chunks_exact(2400)
                .take(320)
                .flat_map(|row| row[..4 * 135].iter().chain(&row[4 * 465..]).copied())
                .collect(),
        })
    };

    let opaque = render(State {
        blended: false,
        ..initial
    })?;
    let never = render(State {
        mode: "never",
        blended: false,
        ..initial
    })?;
    let red = [0xd0, 0x4d, 0x30, 255];
    let blue = [0x28, 0x76, 0xd0, 255];
    assert_eq!(opaque.pixels, [red; 3]);
    assert_eq!(never.pixels, [blue; 3]);
    assert_eq!(opaque.hits, never.hits);
    assert_eq!(opaque.uploads, never.uploads);
    assert_eq!(never.prepass, 0);

    let blended = render(initial)?;
    let always = render(State {
        mode: "always",
        ..initial
    })?;
    let prepass = render(State {
        mode: "opaque-prepass",
        ..initial
    })?;
    assert_eq!(blended.hits, ["stripes", "stripes", "late"]);
    assert_eq!(prepass.hits, blended.hits);
    assert_eq!(always.hits, blended.hits);
    assert_eq!(prepass.prepass, 1);
    assert_eq!(blended.prepass, 0);
    assert_eq!(always.prepass, 0);
    assert_ne!(blended.pixels[0], red);
    assert_eq!(always.pixels[0], red);
    assert_eq!(prepass.pixels[0], red);
    assert_ne!(always.pixels[1], blended.pixels[1]);
    assert_eq!(prepass.pixels[1], blended.pixels[1]);
    assert_eq!(prepass.pixels[2], blended.pixels[2]);
    assert_eq!(always.pixels[2], blended.pixels[2]);
    assert_eq!(prepass.uploads, blended.uploads);

    let smooth = render(State {
        mode: "opaque-prepass",
        smooth: true,
        ..initial
    })?;
    assert_eq!(smooth.pixels, prepass.pixels);
    assert_eq!(smooth.prepass, 1);
    let weighted = render(State {
        mode: "opaque-prepass",
        weighted: true,
        ..initial
    })?;
    assert_eq!(weighted.pixels[0], red);
    assert_eq!(weighted.oit, 1);
    let sorted = render(State {
        mode: "always",
        weighted: true,
        ..initial
    })?;
    assert_eq!(sorted.pixels, always.pixels);
    assert_eq!(sorted.oit, 0);

    let masked = render(State {
        mode: "opaque-prepass",
        masked: true,
        ..initial
    })?;
    assert_eq!(masked.hits, ["stripes", "late", "late"]);
    assert_eq!(masked.pixels[0], red);
    assert_eq!(masked.prepass, 1);
    let glass = render(State {
        mode: "opaque-prepass",
        glass: true,
        ..initial
    })?;
    assert_eq!(glass.captures, 1);
    assert_eq!(glass.prepass, 1);
    assert_eq!(glass.hits, blended.hits);
    let unshadowed = render(State {
        shadows: true,
        ..initial
    })?;
    let shadowed = render(State {
        mode: "opaque-prepass",
        shadows: true,
        ..initial
    })?;
    assert!(shadowed.shadows > unshadowed.shadows);
    assert_ne!(shadowed.shadow_region, unshadowed.shadow_region);
    let no_shadow = render(State {
        mode: "never",
        shadows: true,
        ..initial
    })?;
    assert_eq!(no_shadow.shadow_region, unshadowed.shadow_region);
    Ok(())
}

#[test]
fn material_culling_matches_rendering_rays_and_marquee_for_mirrored_meshes() -> TestResult {
    use material_cull_ui::MaterialCullDemo;
    use xrack_runtime::Hit3D;

    const NAME: &str = "examples/puzzle3d/material_cull.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialCullDemo")?;
    let generated = MaterialCullDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy)]
    struct State {
        mode: &'static str,
        rear: bool,
        slot_override: bool,
        prepass: bool,
        motion: bool,
        shadows: bool,
        perspective: bool,
        mirror_y: bool,
    }
    let initial = State {
        mode: "back",
        rear: false,
        slot_override: false,
        prepass: false,
        motion: false,
        shadows: false,
        perspective: false,
        mirror_y: false,
    };
    struct Frame {
        pixels: [[u8; 4]; 2],
        hits: [Hit3D; 2],
        rows: [Vec<i64>; 2],
        uploads: u64,
        prepasses: u64,
        motion_draws: u64,
        shadow: Vec<u8>,
    }
    let mut render = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing material culling demo")?;
        app.set_mode(state.mode.into());
        app.set_rear(state.rear);
        app.set_slot_override(state.slot_override);
        app.set_prepass(state.prepass);
        app.set_motion(state.motion);
        app.set_shadows(state.shadows);
        app.set_perspective(state.perspective);
        app.set_mirror_y(state.mirror_y);
        for (name, value) in [
            ("mode", Value::String(state.mode.into())),
            ("rear", Value::Bool(state.rear)),
            ("slot-override", Value::Bool(state.slot_override)),
            ("prepass", Value::Bool(state.prepass)),
            ("motion", Value::Bool(state.motion)),
            ("shadows", Value::Bool(state.shadows)),
            ("perspective", Value::Bool(state.perspective)),
            ("mirror-y", Value::Bool(state.mirror_y)),
        ] {
            dynamic.set(name, value)?;
        }

        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png, interpreted.render(&mut preview_gpu)?);
        let after = gpu.scene_draw_stats();
        let hits = [app.get_left_hit(), app.get_right_hit()];
        for (name, hit) in ["left-hit", "right-hit"].into_iter().zip(&hits) {
            assert_eq!(dynamic.get(name)?, Value::Hit3D(hit.clone()));
        }
        let rows = [app.get_all_rows(), app.get_partial_rows()];
        for (name, list) in ["all-rows", "partial-rows"].into_iter().zip(&rows) {
            let dynamic_rows = match dynamic.get(name)? {
                Value::IntList(values) => values.to_vec(),
                Value::IntModel(model) => model.snapshot(),
                _ => return Err(format!("missing interpreted selection {name}").into()),
            };
            assert_eq!(list, &dynamic_rows, "{name}");
        }
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        let columns = if state.perspective {
            [240, 360]
        } else {
            [200, 400]
        };
        let pixels = columns.map(|x| {
            let start = (160 * 600 + x) * 4;
            std::array::from_fn(|channel| raster.rgba[start + channel])
        });
        Ok(Frame {
            pixels,
            hits,
            rows,
            uploads: gpu.scene_mesh_uploads(),
            prepasses: after.material_depth_prepass_draws - before.material_depth_prepass_draws,
            motion_draws: after.motion_vector_draws - before.motion_vector_draws,
            shadow: raster
                .rgba
                .chunks_exact(2400)
                .take(320)
                .flat_map(|row| row[..4 * 150].iter().chain(&row[4 * 455..]).copied())
                .collect(),
        })
    };

    let panel = [0xe4, 0x68, 0x32, 255];
    let clear = [0x22, 0x33, 0x4b, 255];
    let check = |frame: &Frame, visible, rear| {
        assert_eq!(frame.pixels, [if visible { panel } else { clear }; 2]);
        for hit in &frame.hits {
            assert_eq!(hit.valid, visible);
            if visible {
                assert_eq!(hit.object_id, "panels");
                assert!(if rear {
                    hit.normal.z < -0.99
                } else {
                    hit.normal.z > 0.99
                });
            }
        }
        for list in &frame.rows {
            assert_eq!(list.as_slice(), if visible { &[2][..] } else { &[][..] });
        }
    };

    let original = render(initial)?;
    check(&original, true, false);
    let mirrored_parent = render(State {
        mirror_y: true,
        ..initial
    })?;
    check(&mirrored_parent, true, false);
    assert_eq!(mirrored_parent.uploads, original.uploads);
    for (mode, rear, visible) in [
        ("back", true, false),
        ("front", false, false),
        ("front", true, true),
        ("none", false, true),
        ("none", true, true),
    ] {
        let frame = render(State {
            mode,
            rear,
            ..initial
        })?;
        check(&frame, visible, rear);
        assert_eq!(
            frame.uploads, original.uploads,
            "culling changes pipeline state, not mesh buffers"
        );
    }

    check(
        &render(State {
            mode: "front",
            slot_override: true,
            ..initial
        })?,
        true,
        false,
    );
    check(
        &render(State {
            mode: "front",
            perspective: true,
            ..initial
        })?,
        false,
        false,
    );
    check(
        &render(State {
            mode: "front",
            perspective: true,
            rear: true,
            ..initial
        })?,
        true,
        true,
    );

    let prepass = render(State {
        mode: "front",
        prepass: true,
        ..initial
    })?;
    check(&prepass, false, false);
    assert!(prepass.prepasses > 0);
    check(
        &render(State {
            mode: "front",
            prepass: true,
            rear: true,
            ..initial
        })?,
        true,
        true,
    );
    let motion = render(State {
        mode: "front",
        motion: true,
        rear: true,
        ..initial
    })?;
    check(&motion, true, true);
    assert!(motion.motion_draws > 0);

    let shadowed = render(State {
        shadows: true,
        ..initial
    })?;
    let culled_shadow = render(State {
        mode: "front",
        shadows: true,
        ..initial
    })?;
    assert!(
        shadowed
            .shadow
            .iter()
            .zip(&culled_shadow.shadow)
            .any(|(&a, &b)| a.abs_diff(b) > 5)
    );
    Ok(())
}

#[test]
fn material_texture_channels_share_render_picking_and_depth_rules() -> TestResult {
    use material_channels_ui::MaterialChannelsDemo;

    const NAME: &str = "examples/puzzle3d/material_channels.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialChannelsDemo")?;
    let generated = MaterialChannelsDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    struct Frame {
        pixel: [u8; 4],
        hit: String,
        depth_draws: u64,
        meshes: u64,
        mip_levels: u64,
        shadow_passes: u64,
        shadow_region: Vec<u8>,
    }
    let mut render =
        |base: &str, shading: &str, channel: &str, shadows: bool| -> TestResult<Frame> {
            let mut app = weak.upgrade().ok_or("missing material channels demo")?;
            app.set_base_channel(base.into());
            app.set_shading(shading.into());
            app.set_shading_channel(channel.into());
            app.set_shadows(shadows);
            for (name, value) in [
                ("base-channel", base),
                ("shading", shading),
                ("shading-channel", channel),
            ] {
                dynamic.set(name, Value::String(value.into()))?;
            }
            dynamic.set("shadows", Value::Bool(shadows))?;

            let before = gpu.scene_draw_stats();
            let png = aot.render(&mut gpu)?.to_vec();
            assert!(
                png == interpreted.render(&mut preview_gpu)?,
                "different GPU output with {base}/{shading}/{channel} and shadows={shadows}"
            );
            let raster = xrack_assets::decode_png(&png)?;
            assert_eq!(raster.size, [600, 372]);
            let pixel = (160 * 600 + 300) * 4;
            let hit = app.get_center_hit();
            assert_eq!(dynamic.get("center-hit")?, Value::String(hit.clone()));
            let stats = gpu.scene_draw_stats();
            Ok(Frame {
                pixel: std::array::from_fn(|channel| raster.rgba[pixel + channel]),
                hit: hit.to_string(),
                depth_draws: stats.material_depth_prepass_draws
                    - before.material_depth_prepass_draws,
                meshes: gpu.scene_mesh_uploads(),
                mip_levels: stats.material_mip_levels_uploaded,
                shadow_passes: gpu.scene_shadow_passes(),
                shadow_region: if shadows {
                    raster
                        .rgba
                        .chunks_exact(2400)
                        .take(320)
                        .flat_map(|row| row[..4 * 200].iter().chain(&row[4 * 400..]).copied())
                        .collect()
                } else {
                    Vec::new()
                },
            })
        };

    let rgb = render("rgb", "base", "r", false)?;
    assert_eq!(rgb.pixel, [0x22, 0x33, 0x4b, 255]);
    assert_eq!(rgb.hit, "");
    assert!(rgb.mip_levels > 0);
    for (channel, expected) in [("r", 218), ("g", 92), ("b", 26), ("a", 99)] {
        let frame = render(channel, "base", "r", false)?;
        assert_eq!(frame.hit, "panel");
        assert!(frame.depth_draws > 0);
        for value in &frame.pixel[..3] {
            assert!(
                value.abs_diff(expected) <= 1,
                "{channel}: {:?}",
                frame.pixel
            );
        }
        assert_eq!(
            (frame.meshes, frame.mip_levels),
            (rgb.meshes, rgb.mip_levels)
        );
    }

    let red = render("r", "emissive", "r", false)?;
    let green = render("r", "emissive", "g", false)?;
    assert!(
        red.pixel[0] > green.pixel[0] + 50,
        "{:?} vs {:?}",
        red.pixel,
        green.pixel
    );
    assert_eq!(red.mip_levels, green.mip_levels);

    let metallic = render("r", "pbr", "r", false)?;
    let dielectric = render("r", "pbr", "a", false)?;
    assert_ne!(metallic.pixel, dielectric.pixel);
    assert_eq!(
        (metallic.meshes, metallic.mip_levels),
        (dielectric.meshes, dielectric.mip_levels)
    );

    let empty = render("rgb", "base", "r", true)?;
    let shadowed = render("r", "base", "r", true)?;
    assert!(shadowed.shadow_passes > empty.shadow_passes);
    assert!(
        shadowed
            .shadow_region
            .iter()
            .zip(&empty.shadow_region)
            .any(|(&a, &b)| a.abs_diff(b) > 5)
    );
    let same_shadow = render("g", "base", "r", true)?;
    assert_eq!(same_shadow.shadow_passes, shadowed.shadow_passes);
    assert!(same_shadow.shadow_region == shadowed.shadow_region);
    Ok(())
}

#[test]
fn vertex_color_masks_preserve_shared_coverage_and_plan_transmission() -> TestResult {
    use vertex_masks_ui::VertexMasksDemo;

    const NAME: &str = "examples/puzzle3d/vertex_masks.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("VertexMasksDemo")?;
    let generated = VertexMasksDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy, Debug)]
    struct State {
        look: &'static str,
        channel: &'static str,
        tint: bool,
        shadows: bool,
        motion: bool,
    }

    struct Frame {
        pixels: [[u8; 4]; 2],
        hits: [String; 2],
        mesh_uploads: u64,
        shadow_passes: u64,
        prepass_draws: u64,
        motion_draws: u64,
        transmission_captures: u64,
        shadow_region: Vec<u8>,
    }

    let mut render = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing vertex colors demo")?;
        app.set_look(state.look.into());
        app.set_channel(state.channel.into());
        app.set_vertex_tint(state.tint);
        app.set_shadows(state.shadows);
        app.set_motion(state.motion);
        for (name, value) in [
            ("look", Value::String(state.look.into())),
            ("channel", Value::String(state.channel.into())),
            ("vertex-tint", Value::Bool(state.tint)),
            ("shadows", Value::Bool(state.shadows)),
            ("motion", Value::Bool(state.motion)),
        ] {
            dynamic.set(name, value)?;
        }
        let hits = [app.get_left_hit(), app.get_right_hit()];
        for (name, hit) in ["left-hit", "right-hit"].into_iter().zip(&hits) {
            assert_eq!(dynamic.get(name)?, Value::String(hit.clone()));
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png == interpreted.render(&mut preview_gpu)?,
            "different output for {state:?}"
        );
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        let pixel =
            |x: usize| std::array::from_fn(|channel| raster.rgba[(160 * 600 + x) * 4 + channel]);
        let after = gpu.scene_draw_stats();
        Ok(Frame {
            pixels: [pixel(220), pixel(380)],
            hits: hits.map(|hit| hit.to_string()),
            mesh_uploads: gpu.scene_mesh_uploads(),
            shadow_passes: gpu.scene_shadow_passes(),
            prepass_draws: after.material_depth_prepass_draws - before.material_depth_prepass_draws,
            motion_draws: after.motion_vector_draws - before.motion_vector_draws,
            transmission_captures: after.transmission_captures - before.transmission_captures,
            shadow_region: if state.shadows {
                raster
                    .rgba
                    .chunks_exact(2400)
                    .take(320)
                    .skip(265)
                    .flatten()
                    .copied()
                    .collect()
            } else {
                Vec::new()
            },
        })
    };

    let original = State {
        look: "color",
        channel: "r",
        tint: true,
        shadows: false,
        motion: false,
    };
    let tint = render(original)?;
    let untinted = render(State {
        tint: false,
        ..original
    })?;
    assert_eq!(tint.pixels[0], [0x22, 0x33, 0x4b, 255]);
    assert_ne!(tint.pixels[1], untinted.pixels[1]);
    assert_eq!(untinted.pixels, [[0xe0, 0xe4, 0xef, 255]; 2]);
    assert_eq!(tint.hits, ["", "panel"]);
    assert_eq!(untinted.hits, ["panel"; 2]);
    assert!(untinted.prepass_draws > 0);
    assert_eq!(tint.mesh_uploads, untinted.mesh_uploads);
    let tracked = render(State {
        tint: false,
        motion: true,
        ..original
    })?;
    assert!(tracked.motion_draws > 0);

    for look in ["metal", "roughness", "specular"] {
        let red = State {
            look,
            tint: false,
            ..original
        };
        let first = render(red)?;
        let second = render(State {
            channel: "g",
            ..red
        })?;
        let uniform = render(State {
            channel: "none",
            ..red
        })?;
        assert!(
            first.pixels[0][0].abs_diff(first.pixels[1][0]) > 1,
            "{look}: {:?}",
            first.pixels
        );
        for (l, r) in first.pixels[0].into_iter().zip(second.pixels[1]) {
            assert!(l.abs_diff(r) <= 1, "{look}");
        }
        assert!(
            uniform.pixels[0][0].abs_diff(uniform.pixels[1][0]) <= 1,
            "{look}"
        );
        assert_eq!(first.mesh_uploads, second.mesh_uploads);
    }

    let shadow = State {
        look: "metal",
        shadows: true,
        ..original
    };
    let masked_shadow = render(shadow)?;
    let whole_shadow = render(State {
        tint: false,
        ..shadow
    })?;
    let same_shadow = render(State {
        tint: false,
        channel: "g",
        ..shadow
    })?;
    assert!(whole_shadow.shadow_passes > masked_shadow.shadow_passes);
    assert!(
        whole_shadow
            .shadow_region
            .iter()
            .zip(&masked_shadow.shadow_region)
            .any(|(&a, &b)| a.abs_diff(b) > 5)
    );
    assert_eq!(same_shadow.shadow_passes, whole_shadow.shadow_passes);

    let glass = State {
        look: "glass",
        tint: false,
        ..original
    };
    let metal = render(State {
        channel: "none",
        ..glass
    })?;
    let transmissive = render(glass)?;
    assert_eq!(metal.transmission_captures, 0);
    assert_eq!(transmissive.transmission_captures, 1);
    assert_ne!(metal.pixels, transmissive.pixels);
    assert_eq!(metal.mesh_uploads, transmissive.mesh_uploads);
    Ok(())
}

#[test]
fn authored_optics_share_the_imported_pipeline_and_keep_mesh_coordinates() -> TestResult {
    use material_optics_ui::MaterialOpticsDemo;

    const NAME: &str = "examples/puzzle3d/material_optics.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialOpticsDemo")?;
    let generated = MaterialOpticsDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy, Debug)]
    struct State {
        look: &'static str,
        thickness: f64,
        attenuation: f64,
        dispersion: f64,
        specular: f64,
        specular_tint: xrack_runtime::Color,
        occlusion: f64,
        channel: &'static str,
        mapped: bool,
        patterned: bool,
        imported: bool,
        inherit: bool,
        depth_scale: f64,
    }
    struct Frame {
        raster: xrack_assets::Raster,
        volume_captures: u64,
        mesh_uploads: u64,
        mip_levels_uploaded: u64,
    }
    let mut render = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing optics demo")?;
        app.set_look(state.look.into());
        app.set_thickness(state.thickness);
        app.set_attenuation(state.attenuation);
        app.set_dispersion(state.dispersion);
        app.set_specular(state.specular);
        app.set_specular_tint(state.specular_tint);
        app.set_occlusion(state.occlusion);
        app.set_channel(state.channel.into());
        app.set_mapped(state.mapped);
        app.set_patterned(state.patterned);
        app.set_imported(state.imported);
        app.set_inherit(state.inherit);
        app.set_depth_scale(state.depth_scale);
        for (name, value) in [
            ("look", Value::String(state.look.into())),
            ("thickness", Value::Float(state.thickness)),
            ("attenuation", Value::Float(state.attenuation)),
            ("dispersion", Value::Float(state.dispersion)),
            ("specular", Value::Float(state.specular)),
            ("specular-tint", Value::Color(state.specular_tint)),
            ("occlusion", Value::Float(state.occlusion)),
            ("channel", Value::String(state.channel.into())),
            ("mapped", Value::Bool(state.mapped)),
            ("patterned", Value::Bool(state.patterned)),
            ("imported", Value::Bool(state.imported)),
            ("inherit", Value::Bool(state.inherit)),
            ("depth-scale", Value::Float(state.depth_scale)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png == interpreted.render(&mut preview_gpu)?,
            "different optical render for {state:?}"
        );
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        let after = gpu.scene_draw_stats();
        Ok(Frame {
            raster,
            volume_captures: after.volume_depth_captures - before.volume_depth_captures,
            mesh_uploads: gpu.scene_mesh_uploads(),
            mip_levels_uploaded: after.material_mip_levels_uploaded,
        })
    };
    let pixel = |frame: &Frame, [x, y]: [usize; 2]| {
        let offset = (600 * y + x) * 4;
        std::array::from_fn::<_, 4, _>(|channel| frame.raster.rgba[offset + channel])
    };
    let center = |frame: &Frame| pixel(frame, [300, 160]);
    let initial = State {
        look: "glass",
        thickness: 0.0,
        attenuation: 1.0,
        dispersion: 0.0,
        specular: 1.0,
        specular_tint: xrack_runtime::Color(0xffffffff),
        occlusion: 1.0,
        channel: "r",
        mapped: false,
        patterned: false,
        imported: false,
        inherit: false,
        depth_scale: 1.0,
    };
    let thin = render(initial)?;
    let thick = State {
        thickness: 1.6,
        ..initial
    };
    let tinted = render(thick)?;
    assert_eq!(thin.volume_captures, 0);
    assert_eq!(tinted.volume_captures, 1);
    assert_eq!(thin.mesh_uploads, tinted.mesh_uploads);
    assert!(center(&thin)[2].saturating_sub(center(&tinted)[2]) > 40);
    let no_absorption = render(State {
        attenuation: 0.0,
        ..thick
    })?;
    assert_eq!(center(&thin), center(&no_absorption));

    let red = render(State {
        mapped: true,
        ..thick
    })?;
    let alpha = render(State {
        mapped: true,
        channel: "a",
        ..thick
    })?;
    assert!(center(&alpha)[2].saturating_sub(center(&red)[2]) > 30);
    assert_eq!(red.mip_levels_uploaded, alpha.mip_levels_uploaded);
    assert_eq!(red.mesh_uploads, alpha.mesh_uploads);

    let striped = State {
        patterned: true,
        attenuation: 0.0,
        ..thick
    };
    let undispersed = render(striped)?;
    let dispersed = render(State {
        dispersion: 14.0,
        ..striped
    })?;
    let changed = (105..210)
        .flat_map(|y| (210..390).map(move |x| [x, y]))
        .filter(|&p| pixel(&undispersed, p) != pixel(&dispersed, p))
        .count();
    assert!(changed > 300, "{changed} dispersed pixels");
    assert_eq!(undispersed.mesh_uploads, dispersed.mesh_uploads);

    let imported = State {
        imported: true,
        ..thick
    };
    let authored = render(imported)?;
    let inherited = render(State {
        inherit: true,
        ..imported
    })?;
    let blues = |frame: &Frame| [168, 432].map(|x| pixel(frame, [x, 120])[2]);
    let [left, right] = blues(&authored);
    assert!(left.abs_diff(right) <= 1, "{left} vs {right}");
    let [left, right] = blues(&inherited);
    assert!(left.saturating_sub(right) > 10, "{left} vs {right}");
    assert_eq!(authored.mesh_uploads, inherited.mesh_uploads);
    let stretched = render(State {
        depth_scale: 1.5,
        ..imported
    })?;
    assert!(blues(&authored)[0].saturating_sub(blues(&stretched)[0]) > 12);

    let highs = State {
        look: "highlights",
        thickness: 2.0,
        ..initial
    };
    let matte = render(State {
        specular: 0.0,
        ..highs
    })?;
    let white_highlight = render(highs)?;
    assert_eq!(white_highlight.volume_captures, 0);
    assert!(center(&white_highlight)[0].saturating_sub(center(&matte)[0]) > 25);
    let tinted_highlight = render(State {
        specular_tint: xrack_runtime::Color(0x4890e8ff),
        ..highs
    })?;
    let [r, _, b, _] = center(&tinted_highlight);
    assert!(b.saturating_sub(r) > 25, "{b} vs {r}");
    let high_red = render(State {
        mapped: true,
        ..highs
    })?;
    let high_alpha = render(State {
        channel: "a",
        mapped: true,
        ..highs
    })?;
    let [r, _, b, _] = center(&high_red);
    assert!(r.saturating_sub(b) > 20, "{r} vs {b}");
    assert!(r.saturating_sub(center(&high_alpha)[0]) > 25);
    assert_eq!(high_red.mip_levels_uploaded, high_alpha.mip_levels_uploaded);

    let shaded = State {
        look: "occlusion",
        ..initial
    };
    let occluded = render(shaded)?;
    let unoccluded = render(State {
        occlusion: 0.0,
        ..shaded
    })?;
    assert!(center(&unoccluded)[0].saturating_sub(center(&occluded)[0]) > 12);
    Ok(())
}

#[test]
fn fresnel_material_curves_match_backends_and_feed_direct_probe_and_screen_reflections()
-> TestResult {
    use material_fresnel_ui::FresnelDemo;

    const NAME: &str = "examples/puzzle3d/material_fresnel.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("FresnelDemo")?;
    let generated = FresnelDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy, Debug)]
    struct Curve {
        enabled: bool,
        power: f64,
        scale: f64,
        bias: f64,
    }
    #[derive(Clone, Copy, Debug)]
    struct State {
        base: Curve,
        coat: Curve,
        curved: bool,
        probe: bool,
        floor: bool,
        ssr: bool,
    }
    struct Frame {
        raster: xrack_assets::Raster,
        mesh_uploads: u64,
        environment_uploads: u64,
        ssr_resolves: u64,
    }
    let mut render = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing Fresnel demo")?;
        app.set_base_controls(state.base.enabled);
        app.set_base_power(state.base.power);
        app.set_base_scale(state.base.scale);
        app.set_base_bias(state.base.bias);
        app.set_coat_controls(state.coat.enabled);
        app.set_coat_power(state.coat.power);
        app.set_coat_scale(state.coat.scale);
        app.set_coat_bias(state.coat.bias);
        app.set_curved(state.curved);
        app.set_probe(state.probe);
        app.set_floor(state.floor);
        app.set_screen_reflections(state.ssr);
        for (name, value) in [
            ("base-controls", Value::Bool(state.base.enabled)),
            ("base-power", Value::Float(state.base.power)),
            ("base-scale", Value::Float(state.base.scale)),
            ("base-bias", Value::Float(state.base.bias)),
            ("coat-controls", Value::Bool(state.coat.enabled)),
            ("coat-power", Value::Float(state.coat.power)),
            ("coat-scale", Value::Float(state.coat.scale)),
            ("coat-bias", Value::Float(state.coat.bias)),
            ("curved", Value::Bool(state.curved)),
            ("probe", Value::Bool(state.probe)),
            ("floor", Value::Bool(state.floor)),
            ("screen-reflections", Value::Bool(state.ssr)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png == interpreted.render(&mut preview_gpu)?,
            "different Fresnel output for {state:?}"
        );
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        Ok(Frame {
            raster,
            mesh_uploads: gpu.scene_mesh_uploads(),
            environment_uploads: gpu.scene_environment_uploads(),
            ssr_resolves: gpu.scene_draw_stats().screen_reflection_resolves
                - before.screen_reflection_resolves,
        })
    };
    let pixel = |frame: &Frame, x: usize, y: usize| {
        let offset = 4 * (y * 600 + x);
        std::array::from_fn::<_, 4, _>(|channel| frame.raster.rgba[offset + channel])
    };
    let difference =
        |before: &Frame, after: &Frame, x: std::ops::Range<usize>, y: std::ops::Range<usize>| {
            y.flat_map(|y| x.clone().map(move |x| [x, y]))
                .filter(|&[x, y]| {
                    pixel(before, x, y)
                        .into_iter()
                        .zip(pixel(after, x, y))
                        .any(|(before, after)| before.abs_diff(after) > 4)
                })
                .count()
        };
    let standard = Curve {
        enabled: false,
        power: 5.0,
        scale: 1.0,
        bias: 0.0,
    };
    let initial = State {
        base: standard,
        coat: standard,
        curved: false,
        probe: false,
        floor: false,
        ssr: false,
    };
    let direct = render(initial)?;
    let ignored = render(State {
        base: Curve {
            scale: 0.0,
            bias: 0.6,
            ..standard
        },
        coat: Curve {
            scale: 0.0,
            bias: 0.7,
            ..standard
        },
        ..initial
    })?;
    assert_eq!(difference(&direct, &ignored, 0..600, 0..320), 0);
    let bases = render(State {
        base: Curve {
            enabled: true,
            bias: 0.30,
            ..standard
        },
        ..initial
    })?;
    let coats = render(State {
        coat: Curve {
            enabled: true,
            bias: 0.30,
            ..standard
        },
        ..initial
    })?;
    assert!(pixel(&bases, 180, 160)[0].saturating_sub(pixel(&direct, 180, 160)[0]) > 40);
    assert!(pixel(&coats, 420, 160)[0].saturating_sub(pixel(&direct, 420, 160)[0]) > 40);
    assert_eq!(pixel(&bases, 420, 160), pixel(&direct, 420, 160));
    assert_eq!(pixel(&coats, 180, 160), pixel(&direct, 180, 160));
    assert_eq!(direct.mesh_uploads, bases.mesh_uploads);
    assert_eq!(direct.mesh_uploads, coats.mesh_uploads);

    let probe = State {
        curved: true,
        probe: true,
        ..initial
    };
    let normal = render(probe)?;
    let base_power = render(State {
        base: Curve {
            power: 1.0,
            ..standard
        },
        ..probe
    })?;
    let coat_power = render(State {
        coat: Curve {
            power: 1.0,
            ..standard
        },
        ..probe
    })?;
    assert!(difference(&normal, &base_power, 90..270, 65..255) > 500);
    assert!(difference(&normal, &coat_power, 330..510, 65..255) > 500);
    assert_eq!(difference(&normal, &base_power, 330..510, 65..255), 0);
    assert_eq!(difference(&normal, &coat_power, 90..270, 65..255), 0);
    assert_eq!(normal.mesh_uploads, coat_power.mesh_uploads);
    assert_eq!(normal.environment_uploads, coat_power.environment_uploads);

    let floor = State {
        floor: true,
        ..initial
    };
    let without_ssr = render(floor)?;
    let with_ssr = render(State { ssr: true, ..floor })?;
    assert!(with_ssr.ssr_resolves > 0);
    assert!(difference(&without_ssr, &with_ssr, 0..600, 0..320) > 100);
    let zero = State {
        base: Curve {
            enabled: true,
            scale: 0.0,
            ..standard
        },
        ..floor
    };
    let zero_smooth = render(zero)?;
    let zero_ssr = render(State { ssr: true, ..zero })?;
    assert_eq!(difference(&zero_smooth, &zero_ssr, 0..600, 0..320), 0);
    assert_eq!(zero_smooth.mesh_uploads, zero_ssr.mesh_uploads);
    Ok(())
}

#[test]
fn anisotropic_material_rotates_highlights_and_imports_direction_maps_in_both_backends()
-> TestResult {
    use material_anisotropy_ui::MaterialAnisotropyDemo;

    const NAME: &str = "examples/puzzle3d/material_anisotropy.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MaterialAnisotropyDemo")?;
    let generated = MaterialAnisotropyDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    #[derive(Clone, Copy, Debug)]
    struct State {
        strength: f64,
        rotation: f64,
        mapped: bool,
        imported: bool,
        inherit: bool,
        sphere: bool,
        probe: bool,
        coat: bool,
        ao: bool,
        yaw: f64,
    }
    struct Frame {
        raster: xrack_assets::Raster,
        meshes: u64,
        mip_levels: u64,
        environment: u64,
        ao_evaluations: u64,
    }
    let mut render = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing anisotropic material demo")?;
        app.set_strength(state.strength);
        app.set_rotation(state.rotation);
        app.set_mapped(state.mapped);
        app.set_imported(state.imported);
        app.set_inherit(state.inherit);
        app.set_sphere(state.sphere);
        app.set_probe(state.probe);
        app.set_coat(state.coat);
        app.set_ao(state.ao);
        app.set_yaw(state.yaw);
        for (name, value) in [
            ("strength", Value::Float(state.strength)),
            ("rotation", Value::Float(state.rotation)),
            ("mapped", Value::Bool(state.mapped)),
            ("imported", Value::Bool(state.imported)),
            ("inherit", Value::Bool(state.inherit)),
            ("sphere", Value::Bool(state.sphere)),
            ("probe", Value::Bool(state.probe)),
            ("coat", Value::Bool(state.coat)),
            ("ao", Value::Bool(state.ao)),
            ("yaw", Value::Float(state.yaw)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png == interpreted.render(&mut preview_gpu)?,
            "different brushed material output for {state:?}"
        );
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        let after = gpu.scene_draw_stats();
        Ok(Frame {
            raster,
            meshes: gpu.scene_mesh_uploads(),
            mip_levels: after.material_mip_levels_uploaded,
            environment: gpu.scene_environment_uploads(),
            ao_evaluations: after.occlusion_evaluations - before.occlusion_evaluations,
        })
    };
    let pixel = |frame: &Frame, x: usize, y: usize| {
        let offset = 4 * (y * 600 + x);
        std::array::from_fn::<_, 4, _>(|channel| frame.raster.rgba[offset + channel])
    };
    let initial = State {
        strength: 0.85,
        rotation: 0.0,
        mapped: false,
        imported: false,
        inherit: false,
        sphere: false,
        probe: false,
        coat: false,
        ao: false,
        yaw: 0.0,
    };
    let horizontal = render(initial)?;
    let vertical = render(State {
        rotation: 90.0,
        ..initial
    })?;
    assert!(pixel(&horizontal, 300, 120)[0].saturating_sub(pixel(&vertical, 300, 120)[0]) > 30);
    assert_eq!(horizontal.meshes, vertical.meshes);

    let authored = State {
        imported: true,
        mapped: true,
        ..initial
    };
    let direction_map = render(authored)?;
    let inherited = render(State {
        inherit: true,
        ..authored
    })?;
    for (x, y) in [(220, 110), (380, 110), (220, 212), (380, 212)] {
        for (authored, imported) in pixel(&direction_map, x, y)
            .into_iter()
            .zip(pixel(&inherited, x, y))
        {
            assert!(
                authored.abs_diff(imported) <= 1,
                "material and glTF at {x},{y}"
            );
        }
    }
    let isotropic = render(State {
        strength: 0.0,
        ..authored
    })?;
    let top = [220, 380].map(|x| pixel(&direction_map, x, 110)[0]);
    let bottom = [220, 380].map(|x| pixel(&direction_map, x, 212)[0]);
    assert!(top[0].saturating_sub(top[1]) > 30, "{top:?}");
    assert_eq!(bottom, [220, 380].map(|x| pixel(&isotropic, x, 212)[0]));
    assert_eq!(direction_map.meshes, inherited.meshes);

    let combined = State {
        ao: true,
        coat: true,
        ..authored
    };
    let first = render(combined)?;
    let rotated = render(State {
        rotation: 90.0,
        ..combined
    })?;
    assert!(first.ao_evaluations > 0);
    assert_eq!(rotated.ao_evaluations, 0);
    assert!(pixel(&first, 220, 110)[0].abs_diff(pixel(&rotated, 220, 110)[0]) > 25);
    assert_eq!(first.meshes, rotated.meshes);
    assert_eq!(first.mip_levels, rotated.mip_levels);

    let probe = State {
        sphere: true,
        probe: true,
        ..initial
    };
    let before = render(probe)?;
    let after = render(State {
        rotation: 90.0,
        ..probe
    })?;
    let distinct = (70..250)
        .flat_map(|y| (160..440).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            pixel(&before, x, y)
                .into_iter()
                .zip(pixel(&after, x, y))
                .any(|(a, b)| a.abs_diff(b) > 4)
        })
        .count();
    assert!(
        distinct > 500,
        "{distinct} pixels changed in the light probe"
    );
    assert_eq!(before.environment, after.environment);
    assert_eq!(before.meshes, after.meshes);
    Ok(())
}

#[test]
fn parametric_geometry_keeps_rendering_and_collision_in_sync_and_reuses_meshes() -> TestResult {
    use parametric_geometry_ui::ParametricGeometryDemo;

    const NAME: &str = "examples/puzzle3d/parametric_geometry.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ParametricGeometryDemo")?;
    let generated = ParametricGeometryDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    #[derive(Clone, Copy, Debug)]
    struct State {
        shape: &'static str,
        radius: f64,
        resolution: i64,
        offset: f64,
        tint: xrack_runtime::Color,
        shadows: bool,
        duplicate: bool,
        auxiliary: bool,
    }
    struct Frame {
        raster: xrack_assets::Raster,
        center_hit: String,
        ring_hit: String,
        sweep: xrack_runtime::Hit3D,
        uploads: u64,
        submitted_vertices: u64,
        depth_draws: u64,
        motion_draws: u64,
        indexed_uploads: u64,
        geometry_bytes: u64,
        shadow_passes: u64,
    }
    let mut render = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing parametric geometry demo")?;
        app.set_shape(state.shape.into());
        app.set_radius(state.radius);
        app.set_resolution(state.resolution);
        app.set_offset(state.offset);
        app.set_tint(state.tint);
        app.set_shadows(state.shadows);
        app.set_duplicate(state.duplicate);
        app.set_auxiliary(state.auxiliary);
        for (name, value) in [
            ("shape", Value::String(state.shape.into())),
            ("radius", Value::Float(state.radius)),
            ("resolution", Value::Int(state.resolution)),
            ("offset", Value::Float(state.offset)),
            ("tint", Value::Color(state.tint)),
            ("shadows", Value::Bool(state.shadows)),
            ("duplicate", Value::Bool(state.duplicate)),
            ("auxiliary", Value::Bool(state.auxiliary)),
        ] {
            dynamic.set(name, value)?;
        }
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png == interpreted.render(&mut preview_gpu)?,
            "different output for {state:?}"
        );
        let center_hit = app.get_center_hit();
        let ring_hit = app.get_ring_hit();
        let sweep = app.get_center_sweep();
        assert_eq!(
            dynamic.get("center-hit")?,
            Value::String(center_hit.clone())
        );
        assert_eq!(dynamic.get("ring-hit")?, Value::String(ring_hit.clone()));
        assert_eq!(dynamic.get("center-sweep")?, Value::Hit3D(sweep.clone()));
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        let after = gpu.scene_draw_stats();
        Ok(Frame {
            raster,
            center_hit: center_hit.to_string(),
            ring_hit: ring_hit.to_string(),
            sweep,
            uploads: gpu.scene_mesh_uploads(),
            submitted_vertices: after.color_vertices - before.color_vertices,
            depth_draws: after.material_depth_prepass_draws - before.material_depth_prepass_draws,
            motion_draws: after.motion_vector_draws - before.motion_vector_draws,
            indexed_uploads: after.indexed_mesh_uploads,
            geometry_bytes: after.mesh_vertex_bytes_uploaded + after.mesh_index_bytes_uploaded,
            shadow_passes: gpu.scene_shadow_passes(),
        })
    };
    let pixel = |frame: &Frame, x: usize, y: usize| {
        let start = (y * 600 + x) * 4;
        std::array::from_fn::<_, 4, _>(|channel| frame.raster.rgba[start + channel])
    };
    let initial = State {
        shape: "torus",
        radius: 0.9,
        resolution: 32,
        offset: 0.0,
        tint: xrack_runtime::Color(0xec843fff),
        shadows: false,
        duplicate: false,
        auxiliary: false,
    };
    let clear = [0x22, 0x33, 0x4b, 255];
    let warm = render(State {
        shape: "plane",
        resolution: 1,
        ..initial
    })?;
    let ring = render(initial)?;
    assert_eq!(ring.indexed_uploads, warm.indexed_uploads + 1);
    let compact_bytes = ring.geometry_bytes - warm.geometry_bytes;
    let direct_bytes = ring.submitted_vertices * xrack_runtime::scene3d::MeshVertex::STRIDE as u64;
    assert!(
        compact_bytes * 3 < direct_bytes,
        "{compact_bytes} vs {direct_bytes}"
    );
    eprintln!(
        "Procedural torus GPU geometry: {compact_bytes} bytes indexed vs {direct_bytes} bytes expanded"
    );
    assert_eq!(pixel(&ring, 300, 160), clear);
    assert_eq!(pixel(&ring, 372, 160), [0xec, 0x84, 0x3f, 255]);
    assert_eq!(ring.center_hit, "");
    assert_eq!(ring.ring_hit, "piece");
    assert!(!ring.sweep.valid);
    let moved = render(State {
        offset: 0.12,
        ..initial
    })?;
    let tint = render(State {
        tint: xrack_runtime::Color(0x496aaaff),
        ..initial
    })?;
    assert_eq!(moved.uploads, ring.uploads);
    assert_eq!(tint.uploads, ring.uploads);
    assert_eq!(pixel(&tint, 372, 160), [0x49, 0x6a, 0xaa, 255]);

    let copy = render(State {
        duplicate: true,
        ..initial
    })?;
    assert_eq!(copy.uploads, ring.uploads);
    assert_eq!(copy.geometry_bytes, ring.geometry_bytes);
    assert_eq!(copy.submitted_vertices, 2 * ring.submitted_vertices);
    assert_eq!(pixel(&copy, 564, 160), [0x66, 0xb8, 0xff, 255]);
    let coarse = render(State {
        resolution: 7,
        ..initial
    })?;
    assert!(coarse.uploads > copy.uploads);
    assert!(coarse.submitted_vertices < ring.submitted_vertices);
    let restored = render(initial)?;
    assert_eq!(restored.uploads, coarse.uploads);
    assert!(restored.raster.rgba == ring.raster.rgba);

    for shape in ["plane", "box", "sphere", "cylinder", "cone", "capsule"] {
        let solid = render(State { shape, ..initial })?;
        assert_eq!(solid.center_hit, "piece", "{shape}");
        assert!(solid.sweep.valid, "{shape}");
        // The cone begins at Y = 0. Sample a pixel above that edge.
        let sample_y = if shape == "cone" { 120 } else { 160 };
        assert_eq!(
            pixel(&solid, 300, sample_y),
            [0xec, 0x84, 0x3f, 255],
            "{shape}"
        );
    }
    let invalid = render(State {
        shape: "capsule",
        radius: 1.5,
        ..initial
    })?;
    assert_eq!(invalid.center_hit, "");
    assert_eq!(pixel(&invalid, 300, 160), clear);

    let box_state = State {
        shape: "box",
        ..initial
    };
    let solid = render(box_state)?;
    let unused_radius = render(State {
        radius: 1.03,
        ..box_state
    })?;
    assert_eq!(solid.uploads, unused_radius.uploads);

    let shadow_state = State {
        shadows: true,
        ..initial
    };
    let shadow = render(shadow_state)?;
    let next = render(State {
        radius: 1.08,
        ..shadow_state
    })?;
    assert!(shadow.shadow_passes > 0 && next.shadow_passes > shadow.shadow_passes);
    let shadow_diff = (175..312)
        .flat_map(|y| (420..580).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&shadow, x, y)[0].abs_diff(pixel(&next, x, y)[0]) > 5)
        .count();
    assert!(shadow_diff > 100, "{shadow_diff} differing shadow pixels");
    let auxiliary = render(State {
        auxiliary: true,
        ..shadow_state
    })?;
    assert!(auxiliary.depth_draws > 0 && auxiliary.motion_draws > 0);
    assert_eq!(auxiliary.ring_hit, "piece");
    assert_eq!(auxiliary.uploads, next.uploads);
    Ok(())
}

#[test]
fn authored_mesh_tracks_nested_model_edits_and_keeps_rendering_and_collision_in_sync() -> TestResult
{
    use authored_mesh_ui::AuthoredMeshDemo;

    const NAME: &str = "examples/puzzle3d/authored_mesh.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("AuthoredMeshDemo")?;
    let generated = AuthoredMeshDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut app = weak.upgrade().ok_or("missing authored mesh demo")?;
    let world_stamp = || {
        let Value::RecordList(model) = dynamic.get("world")? else {
            return Err(xrack_interpreter::Error::Unsupported(
                "missing world model".into(),
            ));
        };
        Ok(model.model().edit_stamp())
    };
    let outer = world_stamp()?;

    struct Frame {
        raster: xrack_assets::Raster,
        hit: String,
        sweep: xrack_runtime::Hit3D,
        uploads: u64,
        submitted: u64,
    }
    let mut render = |app: &mut AuthoredMeshDemo| -> TestResult<Frame> {
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png == interpreted.render(&mut preview_gpu)?,
            "authored mesh backend mismatch"
        );
        let hit = app.get_corner_hit();
        let sweep = app.get_corner_sweep();
        assert_eq!(dynamic.get("corner-hit")?, Value::String(hit.clone()));
        assert_eq!(dynamic.get("corner-sweep")?, Value::Hit3D(sweep.clone()));
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        Ok(Frame {
            raster,
            hit: hit.to_string(),
            sweep,
            uploads: gpu.scene_mesh_uploads(),
            submitted: gpu.scene_draw_stats().color_vertices - before.color_vertices,
        })
    };
    let pixel = |frame: &Frame, x: usize, y: usize| {
        let start = (y * 600 + x) * 4;
        std::array::from_fn::<_, 4, _>(|channel| frame.raster.rgba[start + channel])
    };
    let clear = [0x22, 0x33, 0x4b, 255];
    let orange = [0xed, 0x99, 0x51, 255];
    let full = render(&mut app)?;
    assert_eq!(pixel(&full, 250, 112), orange);
    assert_eq!(full.hit, "panel");
    assert_eq!(full.sweep.object_id, "panel");
    assert_eq!(full.submitted, 6);

    app.invoke_move_corner();
    dynamic.invoke("move-corner", &[])?;
    assert_eq!(app.get_before_edit(), "panel");
    assert_eq!(app.get_after_edit(), "");
    assert_eq!(dynamic.get("before-edit")?, Value::String("panel".into()));
    assert_eq!(dynamic.get("after-edit")?, Value::String("".into()));
    assert_eq!(world_stamp()?, outer);
    let cut = render(&mut app)?;
    assert_eq!(pixel(&cut, 250, 112), clear);
    assert_eq!(cut.hit, "");
    assert!(!cut.sweep.valid);
    assert_eq!(cut.uploads, full.uploads + 1);

    app.invoke_move_corner();
    dynamic.invoke("move-corner", &[])?;
    assert_eq!(world_stamp()?, outer);
    let restored = render(&mut app)?;
    assert_eq!(pixel(&restored, 250, 112), orange);
    assert_eq!(restored.uploads, cut.uploads);
    assert_eq!(restored.sweep, full.sweep);

    app.invoke_set_last_index(1);
    dynamic.invoke("set-last-index", &[Value::Int(1)])?;
    assert_eq!(world_stamp()?, outer);
    let indexed = render(&mut app)?;
    assert_eq!(pixel(&indexed, 250, 112), clear);
    assert!(!indexed.sweep.valid);
    assert_eq!(indexed.uploads, restored.uploads + 1);

    app.invoke_set_last_index(999);
    dynamic.invoke("set-last-index", &[Value::Int(999)])?;
    let invalid = render(&mut app)?;
    assert_eq!(invalid.submitted, 0);
    assert_eq!(pixel(&invalid, 350, 200), clear);
    assert_eq!(invalid.uploads, indexed.uploads);

    app.invoke_set_last_index(3);
    dynamic.invoke("set-last-index", &[Value::Int(3)])?;
    assert_eq!(world_stamp()?, outer);
    let repaired = render(&mut app)?;
    assert_eq!(repaired.hit, "panel");
    assert_eq!(repaired.sweep, full.sweep);
    assert_eq!(repaired.uploads, indexed.uploads);
    assert_eq!(pixel(&repaired, 250, 112), orange);

    // A copied instance and a color/transform edit only affect the instance resources.
    app.set_duplicate(true);
    dynamic.set("duplicate", Value::Bool(true))?;
    app.set_tint(xrack_runtime::Color(0x77aeffff));
    dynamic.set("tint", Value::Color(xrack_runtime::Color(0x77aeffff)))?;
    app.set_offset(0.1);
    dynamic.set("offset", Value::Float(0.1))?;
    let before = gpu.scene_draw_stats();
    let png = aot.render(&mut gpu)?.to_vec();
    assert!(png == interpreted.render(&mut preview_gpu)?);
    assert_eq!(gpu.scene_mesh_uploads(), repaired.uploads);
    assert_eq!(
        gpu.scene_draw_stats().color_vertices - before.color_vertices,
        12
    );
    Ok(())
}

#[test]
fn authored_subsets_keep_material_picking_and_uv_sets_in_sync_without_vertex_uploads() -> TestResult
{
    use mesh_subsets_ui::MeshSubsetsDemo;

    const NAME: &str = "examples/puzzle3d/mesh_subsets.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("MeshSubsetsDemo")?;
    let generated = MeshSubsetsDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut app = weak.upgrade().ok_or("missing mesh subsets demo")?;
    let world_stamp = || -> TestResult<(u64, u64)> {
        let Value::RecordList(world) = dynamic.get("world")? else {
            return Err("missing world model".into());
        };
        Ok(world.model().edit_stamp())
    };

    struct Frame {
        pixels: [[u8; 4]; 3],
        hits: [String; 2],
        uploads: u64,
        draws: u64,
        submitted: u64,
    }
    let mut render = |app: &mut MeshSubsetsDemo| -> TestResult<Frame> {
        let before = gpu.scene_draw_stats();
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(
            png == interpreted.render(&mut preview_gpu)?,
            "mesh subsets backend mismatch"
        );
        let raster = xrack_assets::decode_png(&png)?;
        assert_eq!(raster.size, [600, 372]);
        let pixels = [164_usize, 300, 436].map(|x| {
            let offset = (160 * 600 + x) * 4;
            std::array::from_fn(|channel| raster.rgba[offset + channel])
        });
        let hits = [app.get_left_hit(), app.get_middle_hit()];
        for (name, hit) in ["left-hit", "middle-hit"].into_iter().zip(&hits) {
            assert_eq!(dynamic.get(name)?, Value::String(hit.clone()));
        }
        let after = gpu.scene_draw_stats();
        Ok(Frame {
            pixels,
            hits: hits.map(|hit| hit.to_string()),
            uploads: gpu.scene_mesh_uploads(),
            draws: after.color_draws - before.color_draws,
            submitted: after.color_vertices - before.color_vertices,
        })
    };

    let badge = [0xd8, 0x72, 0x49, 255];
    let surface = [0xd9, 0xdd, 0xe5, 255];
    let green = [0x19, 0xdc, 0x76, 255];
    let blue = [0x20, 0x52, 0xe8, 255];
    let clear = [0x22, 0x33, 0x4b, 255];
    let initial = render(&mut app)?;
    assert_eq!(initial.pixels, [badge, surface, green]);
    assert_eq!((initial.draws, initial.submitted), (3, 18));

    app.set_secondary_uv(false);
    dynamic.set("secondary-uv", Value::Bool(false))?;
    let uv0 = render(&mut app)?;
    assert_eq!(uv0.pixels, [badge, surface, blue]);
    assert_eq!(uv0.uploads, initial.uploads);
    app.set_secondary_uv(true);
    dynamic.set("secondary-uv", Value::Bool(true))?;

    app.set_hidden_badge(true);
    dynamic.set("hidden-badge", Value::Bool(true))?;
    let masked = render(&mut app)?;
    assert_eq!(masked.pixels, [clear, surface, green]);
    assert_eq!(masked.hits, ["", "tiles"]);
    let outer_stamp = world_stamp()?;
    app.invoke_move_badge();
    dynamic.invoke("move-badge", &[])?;
    assert_eq!(world_stamp()?, outer_stamp);
    let moved = render(&mut app)?;
    assert_eq!(moved.pixels, [surface, clear, green]);
    assert_eq!(moved.hits, ["tiles", ""]);
    assert_eq!(moved.uploads, initial.uploads);

    app.set_hidden_badge(false);
    dynamic.set("hidden-badge", Value::Bool(false))?;
    app.invoke_replace_segment(0, 12, 6, "badge".into());
    dynamic.invoke(
        "replace-segment",
        &[
            Value::Int(0),
            Value::Int(12),
            Value::Int(6),
            Value::String("badge".into()),
        ],
    )?;
    let merged = render(&mut app)?;
    assert_eq!(merged.pixels, [surface, badge, badge]);
    assert_eq!((merged.draws, merged.submitted), (2, 18));
    assert_eq!(merged.uploads, initial.uploads);

    app.invoke_replace_segment(1, 12, 6, "badge".into());
    dynamic.invoke(
        "replace-segment",
        &[
            Value::Int(1),
            Value::Int(12),
            Value::Int(6),
            Value::String("badge".into()),
        ],
    )?;
    let invalid = render(&mut app)?;
    assert_eq!(invalid.pixels, [clear; 3]);
    assert_eq!(invalid.hits, ["", ""]);
    app.invoke_replace_segment(1, 6, 6, "badge".into());
    dynamic.invoke(
        "replace-segment",
        &[
            Value::Int(1),
            Value::Int(6),
            Value::Int(6),
            Value::String("badge".into()),
        ],
    )?;
    let repaired = render(&mut app)?;
    assert_eq!(repaired.pixels, merged.pixels);
    assert_eq!(repaired.uploads, initial.uploads);

    app.set_duplicate(true);
    dynamic.set("duplicate", Value::Bool(true))?;
    let instances = render(&mut app)?;
    assert_eq!(instances.submitted, 2 * initial.submitted);
    assert_eq!(instances.uploads, initial.uploads);
    Ok(())
}

#[test]
fn scene_node_coordinate_mapping_matches_backends_and_layout() -> TestResult {
    use node_mapping_ui::NodeMappingDemo;
    use xrack_runtime::{Point3D, Vector3D};

    const NAME: &str = "examples/puzzle3d/node_mapping.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("NodeMappingDemo")?;
    let generated = NodeMappingDemo::new();
    let weak = generated.as_weak();
    let mut aot = PreviewScene::new(Box::new(generated));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;

    fn close(actual: [f64; 3], expected: [f64; 3]) {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 2e-6),
            "{actual:?} vs {expected:?}"
        );
    }

    let mut render = |yaw: f64, stretch: f64, visible: bool, target: &str| -> TestResult<_> {
        let mut app = weak.upgrade().ok_or("missing node mapping instance")?;
        app.set_yaw(yaw);
        app.set_stretch(stretch);
        app.set_show_rig(visible);
        app.set_destination(target.into());
        for (name, value) in [
            ("yaw", Value::Float(yaw)),
            ("stretch", Value::Float(stretch)),
            ("show-rig", Value::Bool(visible)),
            ("destination", Value::String(target.into())),
        ] {
            dynamic.set(name, value)?;
        }
        for (name, point) in [
            ("socket-world", app.get_socket_world()),
            ("socket-relative", app.get_socket_relative()),
            ("round-trip", app.get_round_trip()),
        ] {
            assert_eq!(dynamic.get(name)?, Value::Point3D(point));
        }
        for (name, direction) in [
            ("aim-world", app.get_aim_world()),
            ("aim-relative", app.get_aim_relative()),
            ("aim-round-trip", app.get_aim_round_trip()),
        ] {
            assert_eq!(dynamic.get(name)?, Value::Vector3D(direction));
        }
        let point = app.get_socket_world();
        let length = (0.8 - 0.25) * stretch;
        let (sin, cos) = yaw.to_radians().sin_cos();
        close(
            [point.x, point.y, point.z],
            [0.65 + length * cos, 0.1125, -length * sin],
        );
        let heading = app.get_aim_world();
        let (sin, cos) = (yaw + 30.0).to_radians().sin_cos();
        close(
            [heading.x, heading.y, heading.z],
            [1.65 * sin, 0.0, 1.65 * cos],
        );
        if target == "absent" {
            assert_eq!(app.get_socket_relative(), Point3D::new(-99.0, -99.0, -99.0));
            assert_eq!(app.get_aim_relative(), Vector3D::new(-99.0, -99.0, -99.0));
        } else {
            let round_trip = app.get_round_trip();
            close([round_trip.x, round_trip.y, round_trip.z], [0.0; 3]);
            let aim = app.get_aim_round_trip();
            close([aim.x, aim.y, aim.z], [0.0, 0.0, 1.65]);
        }
        let png = aot.render(&mut gpu)?.to_vec();
        assert_eq!(png.as_slice(), interpreted.render(&mut gpu)?);
        Ok(png)
    };
    let initial = render(0.0, 1.5, true, "dock")?;
    let rotated = render(62.0, 1.5, true, "dock")?;
    assert!(initial != rotated);
    let scaled = render(62.0, 2.5, true, "")?;
    assert!(rotated != scaled);
    let hidden = render(62.0, 2.5, false, "")?;
    assert!(scaled != hidden);
    let _ = render(62.0, 2.5, false, "absent")?;
    Ok(())
}

#[test]
fn transparent_view_composites_over_two_dimensional_content_across_passes() -> TestResult {
    use transparent_view_ui::TransparentViewDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/transparent_view.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("TransparentViewDemo")?;
    let app = TransparentViewDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy, Debug)]
    struct View {
        clear: &'static str,
        quality: &'static str,
        camera: &'static str,
        alternate: bool,
        objects: bool,
        glass: bool,
        fog: bool,
        sky: bool,
    }
    let initial = View {
        clear: "transparent",
        quality: "normal",
        camera: "eye",
        alternate: false,
        objects: true,
        glass: false,
        fog: false,
        sky: false,
    };
    struct Frame {
        image: xrack_assets::Raster,
        stats: SceneDrawStats,
    }
    impl Frame {
        fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
            self.image.rgba.as_chunks::<4>().0[600 * y + x]
        }
    }
    let backdrop = [116, 84, 72, 255];
    let alternative = [40, 76, 100, 255];
    let mut draw = |view: View| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing transparent view")?;
        app.set_clear_mode(view.clear.into());
        app.set_quality(view.quality.into());
        app.set_camera_id(view.camera.into());
        app.set_alternate(view.alternate);
        app.set_models_visible(view.objects);
        app.set_glass(view.glass);
        app.set_fog(view.fog);
        app.set_sky(view.sky);
        for (name, value) in [
            ("clear-mode", Value::String(view.clear.into())),
            ("quality", Value::String(view.quality.into())),
            ("camera-id", Value::String(view.camera.into())),
            ("alternate", Value::Bool(view.alternate)),
            ("models-visible", Value::Bool(view.objects)),
            ("glass", Value::Bool(view.glass)),
            ("fog", Value::Bool(view.fog)),
            ("sky", Value::Bool(view.sky)),
        ] {
            dynamic.set(name, value)?;
        }
        assert_eq!(dynamic.get("picked")?, Value::String(app.get_picked()));
        assert_eq!(
            app.get_picked(),
            if view.objects && view.camera != "absent" {
                "orb"
            } else {
                ""
            }
        );
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(png == interpreted.render(&mut preview_gpu)?, "{view:?}");
        Ok(Frame {
            image: xrack_assets::decode_png(&png)?,
            stats: gpu.scene_draw_stats(),
        })
    };
    let plain = draw(initial)?;
    assert_eq!(plain.pixel(10, 10), backdrop);
    assert_eq!(plain.pixel(158, 145), [255, 230, 144, 255]);
    assert!(plain.stats.alpha_composite_resolves > 0);
    let changed = draw(View {
        alternate: true,
        ..initial
    })?;
    assert_eq!(changed.pixel(10, 10), alternative);
    assert_eq!(changed.pixel(158, 145), plain.pixel(158, 145));
    assert_ne!(changed.pixel(442, 145), plain.pixel(442, 145));
    assert_eq!(changed.stats.viewport_renders, plain.stats.viewport_renders);
    assert_eq!(
        changed.stats.alpha_composite_resolves,
        plain.stats.alpha_composite_resolves
    );
    assert!(changed.stats.viewport_cache_hits > plain.stats.viewport_cache_hits);

    let close = |actual: [u8; 4], expected: [u8; 4]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "{actual:?} vs {expected:?}"
        );
    };
    let tinted = draw(View {
        clear: "tint",
        ..initial
    })?;
    close(tinted.pixel(10, 10), [142, 74, 52, 255]);
    let opaque = draw(View {
        clear: "opaque",
        ..initial
    })?;
    assert_eq!(opaque.pixel(10, 10), [168, 64, 32, 255]);
    assert_eq!(
        opaque.stats.alpha_composite_resolves,
        tinted.stats.alpha_composite_resolves
    );

    for quality in [
        "msaa",
        "ssaa",
        "hdr",
        "weighted",
        "screen",
        "lut",
        "focus",
        "bloom",
        "progressive",
        "temporal",
        "particles",
    ] {
        let state = View { quality, ..initial };
        let frame = draw(state)?;
        assert_eq!(frame.pixel(10, 10), backdrop, "{quality}");
        let before_controls = 600 * 290 * 4;
        if matches!(quality, "focus" | "bloom" | "screen" | "lut" | "particles") {
            assert!(frame.image.rgba[..before_controls] != plain.image.rgba[..before_controls]);
        }
        match quality {
            "msaa" => assert!(frame.stats.multisample_resolves > 0),
            "ssaa" => assert!(frame.stats.supersample_resolves > 0),
            "hdr" => assert!(frame.stats.hdr_resolves > 0),
            "weighted" => assert!(frame.stats.transparency_composites > 0),
            "screen" => assert!(frame.stats.screen_reflection_resolves > 0),
            "lut" => assert!(frame.stats.lut_resolves > 0),
            "focus" => assert!(frame.stats.depth_of_field_passes > 0),
            "particles" => {
                assert!(frame.stats.particle_draws > 0);
                assert_ne!(frame.pixel(103, 217), backdrop);
            }
            _ => {}
        }
        if quality == "bloom" {
            assert_ne!(frame.pixel(360, 145), backdrop, "glow spans beyond the orb");
        }
        if matches!(quality, "temporal" | "progressive") {
            let next = draw(state)?;
            assert_eq!(next.pixel(10, 10), backdrop, "{quality} history");
            assert!(next.stats.viewport_renders > frame.stats.viewport_renders);
        }
    }

    let glass = draw(View {
        glass: true,
        ..initial
    })?;
    let glass_alt = draw(View {
        glass: true,
        alternate: true,
        ..initial
    })?;
    assert_eq!(glass.pixel(10, 10), backdrop);
    assert!(glass.pixel(442, 145)[0].abs_diff(glass_alt.pixel(442, 145)[0]) > 45);
    assert!(glass.stats.transmission_captures > 0);
    let fog = draw(View {
        fog: true,
        ..initial
    })?;
    assert_ne!(fog.pixel(506, 47), backdrop);
    assert_eq!(fog.pixel(10, 10), backdrop);
    let sky = draw(View {
        sky: true,
        ..initial
    })?;
    let other_sky = draw(View {
        sky: true,
        alternate: true,
        ..initial
    })?;
    assert_ne!(sky.pixel(10, 10), backdrop);
    assert_eq!(sky.pixel(10, 10), other_sky.pixel(10, 10));

    let bars = draw(View {
        camera: "rig/Hero",
        clear: "tint",
        ..initial
    })?;
    close(bars.pixel(10, 10), [142, 74, 52, 255]);
    close(bars.pixel(360, 145), bars.pixel(10, 10));
    let empty = draw(View {
        objects: false,
        ..initial
    })?;
    assert_eq!(empty.pixel(300, 145), backdrop);
    let no_camera = draw(View {
        camera: "absent",
        ..initial
    })?;
    assert_eq!(no_camera.pixel(300, 145), backdrop);
    Ok(())
}

#[test]
fn scene_item_textures_refresh_in_place_and_reuse_gpu_meshes() -> TestResult {
    use item_texture_ui::ItemTextureDemo;
    use xrack_renderer_femtovg::SceneDrawStats;
    use xrack_runtime::Length;

    const NAME: &str = "examples/puzzle3d/item_texture.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ItemTextureDemo")?;
    let app = ItemTextureDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);

    #[derive(Clone, Copy, Debug)]
    struct State {
        active: bool,
        heading: f64,
        visible: bool,
        duplicate: bool,
        width: f64,
        name: &'static str,
    }
    struct Frame {
        image: xrack_assets::Raster,
        stats: SceneDrawStats,
        meshes: u64,
    }
    impl Frame {
        fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
            self.image.rgba.as_chunks::<4>().0[y * 600 + x]
        }
    }

    let mut draw = |state: State| -> TestResult<Frame> {
        let mut app = weak.upgrade().ok_or("missing 2D item source demo")?;
        app.set_active(state.active);
        app.set_camera_turn(state.heading);
        app.set_source_visible(state.visible);
        app.set_duplicate(state.duplicate);
        app.set_panel_width(Length(state.width));
        app.set_texture_name(state.name.into());
        for (name, value) in [
            ("active", Value::Bool(state.active)),
            ("camera-turn", Value::Float(state.heading)),
            ("source-visible", Value::Bool(state.visible)),
            ("duplicate", Value::Bool(state.duplicate)),
            ("panel-width", Value::Length(Length(state.width))),
            ("texture-name", Value::String(state.name.into())),
        ] {
            dynamic.set(name, value)?;
        }
        assert_eq!(dynamic.get("picked")?, Value::String(app.get_picked()));
        let rendered = aot.render(&mut gpu)?.to_vec();
        assert!(
            rendered == interpreted.render(&mut preview_gpu)?,
            "{state:?}"
        );
        let image = xrack_assets::decode_png(&rendered)?;
        assert_eq!(image.size, [600, 350]);
        Ok(Frame {
            image,
            stats: gpu.scene_draw_stats(),
            meshes: gpu.scene_mesh_uploads(),
        })
    };
    let state = State {
        active: false,
        heading: 0.0,
        visible: true,
        duplicate: false,
        width: 174.0,
        name: "dashboard",
    };
    let first = draw(state)?;
    assert_eq!(first.stats.item_texture_allocations, 1);
    assert_eq!(first.stats.item_texture_captures, 1);
    assert_eq!(first.pixel(177, 145), [113, 67, 57, 255]);
    assert_eq!(first.pixel(423, 145), first.pixel(177, 145));
    let repeated = draw(state)?;
    assert_eq!(
        repeated.stats.item_texture_captures,
        first.stats.item_texture_captures
    );
    assert_eq!(
        repeated.stats.viewport_renders,
        first.stats.viewport_renders
    );
    assert!(repeated.stats.item_texture_reuses > first.stats.item_texture_reuses);

    let activated = State {
        active: true,
        ..state
    };
    let changed = draw(activated)?;
    assert_eq!(changed.pixel(177, 145), [56, 92, 115, 255]);
    assert_eq!(changed.pixel(423, 145), changed.pixel(177, 145));
    assert_eq!(
        changed.stats.item_texture_captures,
        first.stats.item_texture_captures + 1
    );
    assert_eq!(
        changed.stats.item_texture_allocations,
        first.stats.item_texture_allocations
    );
    assert_eq!(changed.meshes, first.meshes);

    let tilted = draw(State {
        heading: 8.0,
        ..activated
    })?;
    assert_eq!(
        tilted.stats.item_texture_captures,
        changed.stats.item_texture_captures
    );
    assert!(tilted.stats.viewport_renders > changed.stats.viewport_renders);
    assert!(tilted.image.rgba[..600 * 224 * 4] != changed.image.rgba[..600 * 224 * 4]);
    let wide = draw(State {
        width: 194.0,
        ..activated
    })?;
    assert_eq!(
        wide.stats.item_texture_allocations,
        changed.stats.item_texture_allocations + 1
    );
    assert_eq!(wide.meshes, changed.meshes);

    let ambiguous = draw(State {
        duplicate: true,
        ..activated
    })?;
    assert_eq!(ambiguous.pixel(177, 145), [255; 4]);
    let restore = draw(activated)?;
    assert_eq!(restore.pixel(177, 145), changed.pixel(177, 145));
    let hidden = draw(State {
        visible: false,
        ..activated
    })?;
    assert_eq!(hidden.pixel(177, 145), [255; 4]);
    let unused = draw(State {
        name: "unregistered",
        ..activated
    })?;
    assert_eq!(unused.pixel(177, 145), [255; 4]);
    assert_eq!(
        unused.stats.item_texture_captures,
        hidden.stats.item_texture_captures
    );
    Ok(())
}

#[test]
fn texture_input_routes_buttons_ranges_and_touch_by_surface() -> TestResult {
    use item_texture_ui::ItemTextureDemo;
    use xrack_runtime::{KeyEvent, PointerButton, PointerEvent};

    const NAME: &str = "examples/puzzle3d/item_texture.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ItemTextureDemo")?;
    let app = ItemTextureDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    fn render(
        aot: &mut PreviewScene,
        interpreted: &mut PreviewScene,
        gpu: &mut Renderer,
        preview_gpu: &mut Renderer,
    ) -> TestResult<(Vec<u8>, xrack_renderer_femtovg::SceneDrawStats)> {
        let before = aot.render(gpu)?.to_vec();
        assert!(before == interpreted.render(preview_gpu)?);
        Ok((before, gpu.scene_draw_stats()))
    }
    let state = weak.upgrade().ok_or("missing item texture input demo")?;
    let check = |activations, downs, ups, cancels| -> TestResult {
        for (name, actual, expected) in [
            ("activations", state.get_activations(), activations),
            ("downs", state.get_downs(), downs),
            ("ups", state.get_ups(), ups),
            ("cancels", state.get_cancels(), cancels),
        ] {
            assert_eq!(dynamic.get(name)?, Value::Int(expected));
            assert_eq!(actual, expected, "{name}");
        }
        Ok(())
    };

    let on_face = |center: f32, u: f32, v: f32| Point {
        x: center + (u - 0.5) * (224.0 / 4.2) * 2.5,
        y: 112.0 + (v - 0.5) * (224.0 / 4.2) * 1.62,
    };
    let left = 300.0 - 2.3 * (224.0 / 4.2);
    let right = 300.0 + 2.3 * (224.0 / 4.2);
    let left_button = on_face(left, 0.48, 0.44);
    let right_button = on_face(right, 0.48, 0.44);
    let range_start = on_face(left, 0.24, 0.733);
    let range_end = on_face(left, 0.84, 0.733);
    let badge = on_face(left, 32.0 / 174.0, 18.0 / 102.0);
    let outside = Point { x: 305.0, y: 20.0 };
    let mouse = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent {
            button: PointerButton::Primary,
            ..Default::default()
        },
    };
    let touch = PointerContact {
        id: 2,
        primary: true,
        event: PointerEvent {
            is_touch: true,
            ..mouse.event
        },
    };
    let (initial, initial_stats) = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    for view in [&mut aot, &mut interpreted] {
        assert!(view.press_contact(mouse, left_button, false));
    }
    let (pressed, pressed_stats) = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    assert!(
        xrack_assets::decode_png(&pressed)?.rgba[..600 * 224 * 4]
            != xrack_assets::decode_png(&initial)?.rgba[..600 * 224 * 4],
        "3D shows the source's pressed state"
    );
    assert_eq!(
        initial_stats.item_texture_allocations,
        pressed_stats.item_texture_allocations
    );
    check(0, 0, 0, 0)?;

    for view in [&mut aot, &mut interpreted] {
        view.finish_contact(mouse, left_button, true, false);
    }
    check(1, 0, 0, 0)?;
    let _ = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    for view in [&mut aot, &mut interpreted] {
        assert!(view.press_contact(mouse, left_button, false));
        view.move_contact(mouse, right_button);
        view.finish_contact(mouse, right_button, true, false);
        view.press_contact(touch, right_button, false);
        view.finish_contact(touch, right_button, true, false);
    }
    check(2, 0, 0, 0)?;
    let _ = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;

    for view in [&mut aot, &mut interpreted] {
        assert!(view.press_contact(mouse, range_start, false));
        view.move_contact(mouse, range_end);
        view.finish_contact(mouse, range_end, true, false);
    }
    let level = state.get_level();
    assert_eq!(dynamic.get("level")?, Value::Float(level));
    assert!(level > 78.0 && level <= 100.0, "{level}");
    let _ = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;

    for view in [&mut aot, &mut interpreted] {
        assert!(view.press_contact(touch, badge, false));
        view.finish_contact(touch, badge, true, false);
        assert!(view.press_contact(mouse, badge, false));
        view.move_contact(mouse, outside);
        view.finish_contact(mouse, outside, true, false);
    }
    check(2, 2, 1, 1)?;
    for (property, coordinate, expected) in [
        ("contact-x", state.get_contact_x(), 22.0),
        ("contact-y", state.get_contact_y(), 7.0),
    ] {
        assert_eq!(dynamic.get(property)?, Value::Length(coordinate));
        assert!(
            (coordinate.0 - expected).abs() < 0.05,
            "{property}: {coordinate:?}"
        );
    }

    let mut state = state;
    state.set_blocker(true);
    dynamic.set("blocker", Value::Bool(true))?;
    let _ = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    for view in [&mut aot, &mut interpreted] {
        view.click(left_button);
    }
    assert_eq!(
        state.get_activations(),
        2,
        "a visible non-pickable occluder covers the panel"
    );
    state.set_blocker(false);
    dynamic.set("blocker", Value::Bool(false))?;
    state.set_input_enabled(false);
    dynamic.set("input-enabled", Value::Bool(false))?;
    let _ = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    for view in [&mut aot, &mut interpreted] {
        view.click(left_button);
        view.click(Point { x: 100.0, y: 270.0 });
    }
    assert_eq!(state.get_activations(), 3, "the real 2D button also works");
    assert_eq!(dynamic.get("activations")?, Value::Int(3));
    state.set_input_enabled(true);
    dynamic.set("input-enabled", Value::Bool(true))?;
    state.set_offscreen_only(true);
    dynamic.set("offscreen-only", Value::Bool(true))?;
    let (material_only, _) = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    let rgba = xrack_assets::decode_png(&material_only)?.rgba;
    let pixel = |x, y| rgba.as_chunks::<4>().0[600 * y + x];
    assert_eq!(pixel(25usize, 239usize), [23, 35, 50, 255]);
    assert_eq!(pixel(10usize, 10usize), [37, 61, 82, 255]);
    assert_eq!(pixel(177usize, 145usize), [56, 92, 115, 255]);

    for view in [&mut aot, &mut interpreted] {
        view.click(Point { x: 100.0, y: 270.0 });
        view.click(Point { x: 100.0, y: 43.0 });
    }
    assert_eq!(
        state.get_activations(),
        3,
        "the panel has no direct window hit region"
    );
    for view in [&mut aot, &mut interpreted] {
        view.click(left_button);
    }
    assert_eq!(state.get_activations(), 4);
    let _ = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    let focused_id = |preview: &PreviewScene| {
        let (scene, _, path) = preview.inspect_frame()?;
        scene.at_path(path?)?.element_id().map(str::to_owned)
    };
    assert_eq!(focused_id(&aot).as_deref(), Some("panel-power"));
    assert_eq!(focused_id(&aot), focused_id(&interpreted));
    let space = KeyEvent {
        key: " ".into(),
        ..Default::default()
    };
    for (preview, renderer) in [(&mut aot, &mut gpu), (&mut interpreted, &mut preview_gpu)] {
        let _ = preview.keyboard(renderer, &space, true);
        let _ = preview.keyboard(renderer, &space, false);
    }
    assert_eq!(state.get_activations(), 5);
    assert_eq!(dynamic.get("activations")?, Value::Int(5));

    state.set_input_enabled(false);
    dynamic.set("input-enabled", Value::Bool(false))?;
    let _ = render(&mut aot, &mut interpreted, &mut gpu, &mut preview_gpu)?;
    assert_ne!(focused_id(&aot).as_deref(), Some("panel-power"));
    assert_eq!(focused_id(&aot), focused_id(&interpreted));
    Ok(())
}

#[test]
fn live_item_mipmaps_filter_alpha_coverage_and_keep_warm_gpu_textures() -> TestResult {
    use item_texture_ui::ItemTextureDemo;

    const NAME: &str = "examples/puzzle3d/item_texture.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ItemTextureDemo")?;
    let app = ItemTextureDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut app = weak.upgrade().ok_or("missing live mipmap fixture")?;
    app.set_fine_grid(true);
    app.set_offscreen_only(true);
    dynamic.set("fine-grid", Value::Bool(true))?;
    dynamic.set("offscreen-only", Value::Bool(true))?;

    let mut draw = |app: &mut ItemTextureDemo, mips, height, active| -> TestResult<_> {
        app.set_mipmaps(mips);
        app.set_camera_height(height);
        app.set_active(active);
        dynamic.set("mipmaps", Value::Bool(mips))?;
        dynamic.set("camera-height", Value::Float(height))?;
        dynamic.set("active", Value::Bool(active))?;
        let png = aot.render(&mut gpu)?.to_vec();
        assert!(png == interpreted.render(&mut preview_gpu)?);
        Ok((xrack_assets::decode_png(&png)?, gpu.scene_draw_stats()))
    };
    let colors = |image: &xrack_assets::Raster| {
        let texels = image.rgba.as_chunks::<4>().0;
        (110..115)
            .flat_map(|y| (287..294).map(move |x| texels[y * 600 + x]))
            .collect::<Vec<_>>()
    };
    let spread = |colors: &[[u8; 4]]| -> u8 {
        let channels = colors.iter().map(|rgba| rgba[0]);
        channels.clone().max().unwrap_or_default() - channels.min().unwrap_or_default()
    };
    let (unfiltered, before) = draw(&mut app, false, 54.0, false)?;
    assert_eq!(before.item_texture_mip_passes, 0);
    let (filtered, after) = draw(&mut app, true, 54.0, false)?;
    assert_eq!(after.item_texture_mip_allocations, 1);
    assert_eq!(after.item_texture_mip_passes, 8);
    assert_eq!(
        after.item_texture_allocations,
        before.item_texture_allocations
    );
    let direct = colors(&unfiltered);
    let mip = colors(&filtered);
    assert!(spread(&direct) > 50, "{direct:?}");
    assert!(spread(&mip) < 12, "{mip:?}");
    // Half white premultiplied coverage over the blue viewport. Filtering
    // straight color would leave a dark fringe around the transparent stripes.
    assert!(
        mip.iter().all(|pixel| (179..=203).contains(&pixel[0])),
        "{mip:?}"
    );

    let (_, camera) = draw(&mut app, true, 50.0, false)?;
    assert_eq!(
        camera.item_texture_mip_passes,
        after.item_texture_mip_passes
    );
    assert_eq!(camera.item_texture_captures, after.item_texture_captures);
    let (changed_color, repaint) = draw(&mut app, true, 54.0, true)?;
    assert_eq!(repaint.item_texture_mip_allocations, 1);
    assert_eq!(
        repaint.item_texture_mip_passes,
        after.item_texture_mip_passes + 8
    );
    assert_eq!(
        repaint.item_texture_captures,
        camera.item_texture_captures + 1
    );
    assert_eq!(
        repaint.item_texture_allocations,
        before.item_texture_allocations
    );
    assert!(
        colors(&changed_color)
            .iter()
            .all(|pixel| pixel[0] < 105 && pixel[1] > 145)
    );
    let (_, stable) = draw(&mut app, true, 54.0, true)?;
    assert_eq!(
        stable.item_texture_mip_passes,
        repaint.item_texture_mip_passes
    );
    assert_eq!(stable.viewport_renders, repaint.viewport_renders);
    let (restored, off) = draw(&mut app, false, 54.0, false)?;
    assert_eq!(off.item_texture_mip_passes, stable.item_texture_mip_passes);
    assert_eq!(colors(&restored), direct);
    Ok(())
}

#[test]
fn scene_texture_scroll_preserves_local_positions_and_nested_routes() -> TestResult {
    use item_texture_ui::ItemTextureDemo;
    use std::time::{Duration, Instant};
    use xrack_runtime::{Length, PointerButton, PointerEvent, WheelEvent};

    const NAME: &str = "examples/puzzle3d/item_texture.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ItemTextureDemo")?;
    let app = ItemTextureDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut app = weak.upgrade().ok_or("missing texture wheel example")?;
    app.set_scroll_console(true);
    app.set_offscreen_only(true);
    app.set_accept_wheel(true);
    for (name, value) in [
        ("scroll-console", true),
        ("offscreen-only", true),
        ("accept-wheel", true),
    ] {
        dynamic.set(name, Value::Bool(value))?;
    }

    struct SceneInputs<'a> {
        aot: &'a mut PreviewScene,
        interpreted: &'a mut PreviewScene,
        gpu: &'a mut Renderer,
        preview_gpu: &'a mut Renderer,
    }
    impl SceneInputs<'_> {
        fn draw(&mut self) -> TestResult<Vec<u8>> {
            let png = self.aot.render(self.gpu)?.to_vec();
            assert!(png == self.interpreted.render(self.preview_gpu)?);
            Ok(png)
        }

        fn wheel(&mut self, point: Point, delta: f64) {
            let event = WheelEvent {
                delta_y: Length(delta),
                ..Default::default()
            };
            self.aot.wheel_event_with_renderer(self.gpu, point, event);
            self.interpreted
                .wheel_event_with_renderer(self.preview_gpu, point, event);
        }
    }
    let mut inputs = SceneInputs {
        aot: &mut aot,
        interpreted: &mut interpreted,
        gpu: &mut gpu,
        preview_gpu: &mut preview_gpu,
    };
    let on_face = |direction: f32, x: f32, y: f32| Point {
        x: 300.0 + (direction * 2.3 + (x / 174.0 - 0.5) * 2.5) * (224.0 / 4.2),
        y: 112.0 + (y / 102.0 - 0.5) * 1.62 * (224.0 / 4.2),
    };
    let inner = on_face(-1.0, 75.0, 20.0);
    let slider_left = on_face(-1.0, 75.0, 79.0);
    let slider_right = on_face(1.0, 75.0, 79.0);
    let assert_offsets = |app: &ItemTextureDemo, expected: [f64; 3]| -> TestResult {
        for (name, actual, value) in [
            ("inner-offset", app.get_inner_offset(), expected[0]),
            ("outer-offset", app.get_outer_offset(), expected[1]),
            ("viewport-offset", app.get_viewport_offset(), expected[2]),
        ] {
            assert_eq!(dynamic.get(name)?, Value::Length(actual));
            assert!(
                (actual.0 - value).abs() < 0.05,
                "{name}: {actual:?} vs {value}"
            );
        }
        Ok(())
    };
    let baseline = inputs.draw()?;
    inputs.wheel(inner, 72.0);
    let _ = inputs.draw()?;
    assert_eq!(app.get_wheel_events(), 1);
    for (name, actual, expected) in [
        ("wheel-x", app.get_wheel_x(), 75.0),
        ("wheel-y", app.get_wheel_y(), 20.0),
        ("wheel-delta", app.get_wheel_delta(), 72.0),
    ] {
        assert_eq!(dynamic.get(name)?, Value::Length(actual));
        assert!((actual.0 - expected).abs() < 0.05, "{name}: {actual:?}");
    }
    assert_offsets(&app, [0.0; 3])?;
    app.set_accept_wheel(false);
    dynamic.set("accept-wheel", Value::Bool(false))?;
    let _ = inputs.draw()?;
    inputs.wheel(inner, 72.0);
    assert!(inputs.draw()? != baseline);
    assert_offsets(&app, [-28.0, -44.0, 0.0])?;
    inputs.wheel(inner, 100.0);
    let _ = inputs.draw()?;
    assert_offsets(&app, [-28.0, -100.0, -44.0])?;

    let reset = |app: &mut ItemTextureDemo| -> TestResult {
        app.set_inner_offset(Length(0.0));
        app.set_outer_offset(Length(0.0));
        app.set_viewport_offset(Length(0.0));
        for name in ["inner-offset", "outer-offset", "viewport-offset"] {
            dynamic.set(name, Value::Length(Length(0.0)))?;
        }
        Ok(())
    };
    reset(&mut app)?;
    let _ = inputs.draw()?;
    inputs.wheel(slider_left, -28.0);
    assert_eq!(app.get_level(), 17.0);
    assert_eq!(dynamic.get("level")?, Value::Float(17.0));
    inputs.wheel(slider_left, -14.0);
    inputs.wheel(slider_right, -14.0);
    assert_eq!(
        app.get_level(),
        17.0,
        "fractional steps stay on their own surface"
    );
    inputs.wheel(slider_right, -14.0);
    assert_eq!(app.get_level(), 18.0);
    assert_eq!(dynamic.get("level")?, Value::Float(18.0));
    assert_offsets(&app, [0.0; 3])?;

    app.set_blocker(true);
    dynamic.set("blocker", Value::Bool(true))?;
    let _ = inputs.draw()?;
    let events = app.get_wheel_events();
    inputs.wheel(inner, 20.0);
    let _ = inputs.draw()?;
    assert_eq!(app.get_wheel_events(), events);
    assert_eq!(dynamic.get("wheel-events")?, Value::Int(events));
    assert_offsets(&app, [0.0, 0.0, -20.0])?;

    reset(&mut app)?;
    let _ = inputs.draw()?;
    inputs.wheel(Point { x: 110.0, y: 18.0 }, 20.0);
    let _ = inputs.draw()?;
    assert_eq!(app.get_wheel_events(), events);
    assert_offsets(&app, [0.0, 0.0, -20.0])?;

    app.set_blocker(false);
    dynamic.set("blocker", Value::Bool(false))?;
    let start = on_face(-1.0, 98.0, 48.0);
    let jitter = on_face(-1.0, 98.0, 44.0);
    let end = on_face(-1.0, 98.0, 4.0);
    let face_scale = (224.0 / 4.2) * 1.62 / 102.0;
    let outer = 44.0 - 8.0 / face_scale - 28.0;
    let touch = PointerContact {
        id: 1,
        primary: true,
        event: PointerEvent {
            is_touch: true,
            button: PointerButton::Primary,
            ..Default::default()
        },
    };
    let middle = PointerContact {
        id: 2,
        event: PointerEvent {
            is_touch: false,
            button: PointerButton::Middle,
            ..touch.event
        },
        ..touch
    };
    for contact in [touch, middle] {
        reset(&mut app)?;
        let _ = inputs.draw()?;
        let allocations = inputs.gpu.scene_draw_stats().item_texture_allocations;
        for view in [&mut *inputs.aot, &mut *inputs.interpreted] {
            assert!(view.press_contact(contact, start, !contact.event.is_touch));
            view.move_contact(contact, jitter);
        }
        let _ = inputs.draw()?;
        assert_offsets(&app, [0.0; 3])?;
        for view in [&mut *inputs.aot, &mut *inputs.interpreted] {
            assert!(view.move_contact(contact, end));
            view.finish_contact(contact, end, false, false);
        }
        let _ = inputs.draw()?;
        assert_offsets(&app, [-28.0, -outer, 0.0])?;
        assert_eq!(
            inputs.gpu.scene_draw_stats().item_texture_allocations,
            allocations
        );
        let later = Instant::now() + Duration::from_millis(25);
        inputs.aot.advance_at(inputs.gpu, later);
        inputs.interpreted.advance_at(inputs.preview_gpu, later);
        assert!(app.get_outer_offset().0 < -outer - 1.0);
        assert!(
            matches!(dynamic.get("outer-offset")?, Value::Length(offset) if offset.0 < -outer - 1.0)
        );
        assert_eq!(app.get_viewport_offset(), Length(0.0));
        assert_eq!(dynamic.get("viewport-offset")?, Value::Length(Length(0.0)));
    }

    // Crossing onto the duplicated material cannot retarget the drag or start a fling.
    reset(&mut app)?;
    let _ = inputs.draw()?;
    let other_face = on_face(1.0, 98.0, 88.0);
    for view in [&mut *inputs.aot, &mut *inputs.interpreted] {
        assert!(view.press_contact(touch, start, false));
        assert!(view.move_contact(touch, end));
        view.move_contact(touch, other_face);
        view.move_contact(touch, start);
        view.finish_contact(touch, start, true, false);
    }
    let later = Instant::now() + Duration::from_millis(25);
    inputs.aot.advance_at(inputs.gpu, later);
    inputs.interpreted.advance_at(inputs.preview_gpu, later);
    assert_offsets(&app, [-28.0, -outer, 0.0])?;
    assert_eq!(app.get_activations(), 0);
    assert_eq!(dynamic.get("activations")?, Value::Int(0));
    reset(&mut app)?;
    let _ = inputs.draw()?;
    for view in [&mut *inputs.aot, &mut *inputs.interpreted] {
        assert!(view.press_contact(touch, start, false));
        view.finish_contact(touch, start, true, false);
    }
    let _ = inputs.draw()?;
    assert_eq!(app.get_activations(), 1);
    assert_eq!(dynamic.get("activations")?, Value::Int(1));
    Ok(())
}

#[test]
fn custom_scene_effect_reads_depth_and_reuses_pipelines_in_both_backends() -> TestResult {
    use custom_effect_ui::CustomEffectDemo;
    use xrack_assets::{Raster, decode_png};
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/custom_effect.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|path| path.ends_with("effects/hologram.wgsl"))
    );
    let dynamic = program.create("CustomEffectDemo")?;
    let app = CustomEffectDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<(Raster, SceneDrawStats)> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let stats = gpu.scene_draw_stats();
        let interpreted_stats = preview_gpu.scene_draw_stats();
        assert_eq!(
            stats.custom_effect_passes,
            interpreted_stats.custom_effect_passes
        );
        assert_eq!(
            stats.custom_effect_pipelines,
            interpreted_stats.custom_effect_pipelines
        );
        Ok((decode_png(png)?, stats))
    };
    let pixel = |image: &Raster, x: u32, y: u32| -> [u8; 4] {
        let at = ((y * image.size[0] + x) * 4) as usize;
        image.rgba[at..at + 4].try_into().expect("four channels")
    };
    let mut app = weak.upgrade().ok_or("missing effect demo")?;
    app.set_enabled_effect(false);
    dynamic.set("enabled-effect", Value::Bool(false))?;
    let (plain, stats) = draw()?;
    let original = pixel(&plain, 240, 122);
    let background = pixel(&plain, 56, 44);
    assert_eq!(stats.custom_effect_passes, 0);
    assert_eq!(stats.custom_effect_pipelines, 0);

    app.set_enabled_effect(true);
    app.set_use_depth(false);
    dynamic.set("enabled-effect", Value::Bool(true))?;
    dynamic.set("use-depth", Value::Bool(false))?;
    let (color_only, stats) = draw()?;
    assert_eq!(pixel(&color_only, 240, 122), original);
    assert_eq!(pixel(&color_only, 56, 44), background);
    assert_eq!(stats.custom_effect_passes, 1);
    assert_eq!(stats.custom_effect_pipelines, 1);

    app.set_use_depth(true);
    dynamic.set("use-depth", Value::Bool(true))?;
    let (hologram, stats) = draw()?;
    let tinted = pixel(&hologram, 240, 122);
    assert!(
        tinted[0] < original[0] && tinted[2] > original[2],
        "{original:?} vs {tinted:?}"
    );
    assert_eq!(pixel(&hologram, 56, 44), background);
    assert_eq!(stats.custom_effect_passes, 2);
    assert_eq!(stats.custom_effect_pipelines, 2);
    let (_, reused) = draw()?;
    assert_eq!(reused.custom_effect_passes, stats.custom_effect_passes);
    assert_eq!(reused.viewport_renders, stats.viewport_renders);

    app.set_amount(0.5);
    dynamic.set("amount", Value::Float(0.5))?;
    let (half, stats) = draw()?;
    let mixed = pixel(&half, 240, 122);
    assert!(original[2] < mixed[2] && mixed[2] < tinted[2]);
    assert_ne!(mixed, original);
    assert_eq!(stats.custom_effect_passes, 3);
    assert_eq!(stats.custom_effect_pipelines, 2);

    app.set_msaa(true);
    dynamic.set("msaa", Value::Bool(true))?;
    let (multisampled, stats) = draw()?;
    assert!(pixel(&multisampled, 240, 122)[2] > original[2]);
    assert_eq!(pixel(&multisampled, 56, 44), background);
    assert_eq!(stats.custom_effect_passes, 4);
    assert_eq!(stats.custom_effect_pipelines, 3);
    assert!(stats.multisample_resolves > 0);

    app.set_amount(0.0);
    dynamic.set("amount", Value::Float(0.0))?;
    let (disabled, stats) = draw()?;
    assert_eq!(pixel(&disabled, 240, 122), original);
    assert_eq!(stats.custom_effect_passes, 4);
    assert_eq!(stats.custom_effect_pipelines, 3);
    Ok(())
}

#[test]
fn scene_effect_chain_reuses_half_resolution_targets_and_composites_the_original() -> TestResult {
    use effect_chain_ui::EffectChainDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/effect_chain.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for name in ["effects/blur.wgsl", "effects/halo.wgsl"] {
        assert!(inputs.iter().any(|path| path.ends_with(name)));
    }
    let dynamic = program.create("EffectChainDemo")?;
    let app = EffectChainDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<(Vec<u8>, SceneDrawStats)> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let stats = gpu.scene_draw_stats();
        let interpreted_stats = preview_gpu.scene_draw_stats();
        assert_eq!(
            stats.custom_effect_passes,
            interpreted_stats.custom_effect_passes
        );
        assert_eq!(
            stats.custom_effect_pixels,
            interpreted_stats.custom_effect_pixels
        );
        assert_eq!(
            stats.custom_effect_target_allocations,
            interpreted_stats.custom_effect_target_allocations,
        );
        assert_eq!(
            stats.custom_effect_pipelines,
            interpreted_stats.custom_effect_pipelines
        );
        let raster = xrack_assets::decode_png(png)?;
        assert_eq!(raster.size, [481, 310]);
        Ok((raster.rgba[..481 * 245 * 4].to_vec(), stats))
    };
    let mut app = weak.upgrade().ok_or("missing scene effect chain demo")?;
    app.set_apply_effects(false);
    dynamic.set("apply-effects", Value::Bool(false))?;
    let (original, baseline) = draw()?;
    assert_eq!(baseline.custom_effect_passes, 0);
    assert_eq!(baseline.custom_effect_target_allocations, 0);

    app.set_apply_effects(true);
    app.set_composite(false);
    dynamic.set("apply-effects", Value::Bool(true))?;
    dynamic.set("composite", Value::Bool(false))?;
    let (blurred, blur) = draw()?;
    const HALF_PIXELS: u64 = 241 * 123;
    const FULL_PIXELS: u64 = 481 * 245;
    assert_eq!(blur.custom_effect_passes, 2);
    assert_eq!(blur.custom_effect_pixels, 2 * HALF_PIXELS);
    assert_eq!(blur.custom_effect_pipelines, 1);
    assert_eq!(blur.custom_effect_target_allocations, 2);
    assert_ne!(blurred, original);

    app.set_composite(true);
    dynamic.set("composite", Value::Bool(true))?;
    let (halo, composed) = draw()?;
    assert_eq!(composed.custom_effect_passes - blur.custom_effect_passes, 3);
    assert_eq!(
        composed.custom_effect_pixels - blur.custom_effect_pixels,
        2 * HALF_PIXELS + FULL_PIXELS
    );
    assert_eq!(composed.custom_effect_pipelines, 2);
    assert_eq!(composed.custom_effect_target_allocations, 3);
    let center = (122 * 481 + 240) * 4;
    assert_eq!(&halo[center..center + 4], &original[center..center + 4]);
    assert!(
        original
            .chunks_exact(4)
            .zip(halo.chunks_exact(4))
            .any(|(before, after)| { before == [36, 57, 78, 255] && after[2] > before[2] + 12 }),
        "composite should add a blue halo outside the original geometry"
    );

    let (_, cached) = draw()?;
    assert_eq!(cached.custom_effect_passes, composed.custom_effect_passes);
    assert_eq!(cached.viewport_renders, composed.viewport_renders);
    app.set_radius(14.0);
    dynamic.set("radius", Value::Float(14.0))?;
    let (wider, active) = draw()?;
    assert_ne!(wider, halo);
    assert_eq!(
        active.custom_effect_pipelines,
        composed.custom_effect_pipelines
    );
    assert_eq!(
        active.custom_effect_target_allocations,
        composed.custom_effect_target_allocations
    );

    app.set_composite_first(true);
    dynamic.set("composite-first", Value::Bool(true))?;
    let (reordered, active) = draw()?;
    assert_ne!(reordered, wider);
    assert_eq!(
        active.custom_effect_pipelines,
        composed.custom_effect_pipelines
    );
    assert_eq!(
        active.custom_effect_target_allocations,
        composed.custom_effect_target_allocations
    );
    Ok(())
}

#[test]
fn named_effect_buffers_survive_intervening_passes_and_release_after_the_last_read() -> TestResult {
    use named_effect_buffers_ui::NamedEffectBuffersDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/named_effect_buffers.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let interpreted_instance = program.create("NamedEffectBuffersDemo")?;
    let app = NamedEffectBuffersDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(interpreted_instance.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<(Vec<u8>, SceneDrawStats)> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let stats = gpu.scene_draw_stats();
        let interpreted_stats = preview_gpu.scene_draw_stats();
        assert_eq!(
            stats.custom_effect_passes,
            interpreted_stats.custom_effect_passes
        );
        assert_eq!(
            stats.custom_effect_pipelines,
            interpreted_stats.custom_effect_pipelines
        );
        assert_eq!(
            stats.custom_effect_target_allocations,
            interpreted_stats.custom_effect_target_allocations
        );
        let raster = xrack_assets::decode_png(png)?;
        Ok((raster.rgba[..481 * 245 * 4].to_vec(), stats))
    };
    let center = (122 * 481 + 240) * 4;
    let mut app = weak.upgrade().ok_or("missing named effect buffers demo")?;
    let (cool, initial) = draw()?;
    assert!(cool[center + 2] > cool[center] + 25);
    assert_eq!(initial.custom_effect_passes, 5);
    // The fifth pass can overwrite the first pass's full-size scratch texture.
    assert_eq!(initial.custom_effect_target_allocations, 4);
    assert_eq!(initial.custom_effect_pipelines, 3);

    app.set_tint_amount(0.95);
    interpreted_instance.set("tint-amount", Value::Float(0.95))?;
    let (stronger, refreshed) = draw()?;
    assert!(stronger[center + 2] > cool[center + 2]);
    assert_eq!(
        refreshed.custom_effect_target_allocations,
        initial.custom_effect_target_allocations
    );
    assert_eq!(
        refreshed.custom_effect_pipelines,
        initial.custom_effect_pipelines
    );

    app.set_sharp_input(false);
    interpreted_instance.set("sharp-input", Value::Bool(false))?;
    let (blurred, routed) = draw()?;
    assert_ne!(blurred, stronger);
    assert_eq!(
        routed.custom_effect_target_allocations,
        initial.custom_effect_target_allocations
    );

    app.set_sharp_input(true);
    app.set_use_accent(false);
    interpreted_instance.set("sharp-input", Value::Bool(true))?;
    interpreted_instance.set("use-accent", Value::Bool(false))?;
    let (warm, fallback) = draw()?;
    assert!(warm[center] > warm[center + 2] + 25);
    assert_eq!(
        fallback.custom_effect_passes - routed.custom_effect_passes,
        4
    );
    assert_eq!(
        fallback.custom_effect_target_allocations,
        initial.custom_effect_target_allocations
    );
    assert_eq!(
        fallback.custom_effect_pipelines,
        initial.custom_effect_pipelines
    );
    Ok(())
}

#[test]
fn scene_effect_history_matches_between_backends_and_reuses_two_textures() -> TestResult {
    use effect_history_ui::EffectHistoryDemo;

    const NAME: &str = "examples/puzzle3d/effect_history.xr";
    const BACKGROUND: [u8; 4] = [36, 57, 78, 255];
    const LEFT: u32 = 170;
    const RIGHT: u32 = 310;

    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("EffectHistoryDemo")?;
    let app = EffectHistoryDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<(xrack_assets::Raster, [u64; 2], bool)> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let work = |gpu: &Renderer| {
            [
                gpu.scene_draw_stats().custom_effect_passes,
                gpu.scene_draw_stats().custom_effect_history_allocations,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        assert_eq!(gpu.needs_redraw(), preview_gpu.needs_redraw());
        Ok((
            xrack_assets::decode_png(png)?,
            work(&gpu),
            gpu.needs_redraw(),
        ))
    };
    let pixel = |image: &xrack_assets::Raster, x: u32| -> [u8; 4] {
        let offset = ((121 * image.size[0] + x) * 4) as usize;
        image.rgba[offset..offset + 4]
            .try_into()
            .expect("RGBA texel")
    };
    let mut app = weak.upgrade().ok_or("missing history demo")?;
    let (initial, work, pending) = draw()?;
    assert_eq!(work, [1, 1]);
    assert!(!pending);
    assert_ne!(pixel(&initial, LEFT), BACKGROUND);
    assert_eq!(pixel(&initial, RIGHT), BACKGROUND);
    assert_eq!(
        draw()?.1,
        work,
        "a cached idle view does not advance feedback"
    );

    app.set_object_x(1.4);
    dynamic.set("object-x", Value::Float(1.4))?;
    let (moved, work, _) = draw()?;
    let ghost = pixel(&moved, LEFT)[0];
    assert!(ghost > BACKGROUND[0] + 30);
    assert_ne!(pixel(&moved, RIGHT), BACKGROUND);
    assert_eq!(work, [2, 2]);

    app.set_animate_history(true);
    dynamic.set("animate-history", Value::Bool(true))?;
    let (fade, work, pending) = draw()?;
    assert!(pixel(&fade, LEFT)[0] < ghost);
    assert_eq!(work, [3, 2]);
    assert!(pending);
    let (further, work, pending) = draw()?;
    assert!(pixel(&further, LEFT)[0] < pixel(&fade, LEFT)[0]);
    assert_eq!(work, [4, 2]);
    assert!(pending);

    app.set_animate_history(false);
    app.set_history_revision(1);
    dynamic.set("animate-history", Value::Bool(false))?;
    dynamic.set("history-revision", Value::Int(1))?;
    let (reset, work, pending) = draw()?;
    assert_eq!(pixel(&reset, LEFT), BACKGROUND);
    assert_eq!(work, [5, 3]);
    assert!(!pending);

    app.set_object_x(-1.4);
    dynamic.set("object-x", Value::Float(-1.4))?;
    let (moved, _, _) = draw()?;
    assert_ne!(pixel(&moved, RIGHT), BACKGROUND);
    app.set_view_width(xrack_runtime::Length(440.0));
    dynamic.set("view-width", Value::Length(xrack_runtime::Length(440.0)))?;
    let (resized, work, _) = draw()?;
    assert_eq!(pixel(&resized, RIGHT), BACKGROUND);
    assert_eq!(work, [7, 5]);
    app.set_object_x(1.4);
    dynamic.set("object-x", Value::Float(1.4))?;
    let (moved, work, _) = draw()?;
    assert_ne!(pixel(&moved, 150), BACKGROUND);
    assert_eq!(work, [8, 6]);
    app.set_history_format("rgba8".into());
    dynamic.set("history-format", Value::String("rgba8".into()))?;
    let (converted, work, _) = draw()?;
    assert_eq!(pixel(&converted, 150), BACKGROUND);
    assert_eq!(work, [9, 7]);
    Ok(())
}

#[test]
fn aborted_effect_frame_and_other_viewport_generations_do_not_publish_history() -> TestResult {
    use effect_history_ui::EffectHistoryDemo;
    use xrack_layout::Interaction;
    use xrack_runtime::Length;

    let mut app = EffectHistoryDemo::new();
    let mut gpu = pollster::block_on(Renderer::new(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let interaction = Interaction::default();
    let pixel = |rgba: &[u8], x: usize| -> [u8; 4] {
        let offset = (121 * 480 + x) * 4;
        rgba[offset..offset + 4].try_into().expect("RGBA texel")
    };
    let background = [36, 57, 78, 255];
    gpu.set_scene_observation(700, false);
    gpu.snapshot(&app.render(), 480, 310, 1.0, &interaction)?;

    app.set_object_x(1.4);
    let registration = xrack_renderer_femtovg::register_gpu("abort-effect-history", |_| {
        Err("failed after recording history output".into())
    })?;
    let mut failed = app.render();
    let mut broken = Node::new("CustomRender", Some("abort"));
    for (name, value) in [
        ("renderer", Value::String("abort-effect-history".into())),
        ("width", Value::Length(Length(12.0))),
        ("height", Value::Length(Length(12.0))),
        ("z", Value::Float(200.0)),
    ] {
        broken.properties.insert(name, value);
    }
    failed.children.push(broken);
    let aborted = gpu
        .snapshot(&failed, 480, 310, 1.0, &interaction)
        .err()
        .ok_or("expected failed frame")?;
    assert!(aborted.contains("failed after recording history output"));
    assert_eq!(gpu.scene_draw_stats().custom_effect_passes, 2);
    drop(registration);

    app.set_object_x(0.0);
    let recovered = gpu.snapshot(&app.render(), 480, 310, 1.0, &interaction)?;
    assert_ne!(pixel(&recovered.rgba, 170), background);
    assert_eq!(pixel(&recovered.rgba, 310), background);

    gpu.set_scene_observation(701, false);
    let new_generation = gpu.snapshot(&app.render(), 480, 310, 1.0, &interaction)?;
    assert_eq!(pixel(&new_generation.rgba, 170), background);
    assert_eq!(pixel(&new_generation.rgba, 310), background);

    gpu.set_scene_observation(700, false);
    let old_generation = gpu.snapshot(&app.render(), 480, 310, 1.0, &interaction)?;
    assert_eq!(old_generation.rgba, recovered.rgba);
    Ok(())
}

#[test]
fn scene_effect_data_formats_and_independent_sampling_match_between_backends() -> TestResult {
    use effect_buffer_formats_ui::EffectBufferFormatsDemo;
    use xrack_renderer_femtovg::SceneDrawStats;

    const NAME: &str = "examples/puzzle3d/effect_buffer_formats.xr";
    const FIELD_PIXELS: u64 = 120 * 60;
    const OUTPUT_PIXELS: u64 = 480 * 240;
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("EffectBufferFormatsDemo")?;
    let app = EffectBufferFormatsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<(xrack_assets::Raster, SceneDrawStats)> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let stats = gpu.scene_draw_stats();
        let other = preview_gpu.scene_draw_stats();
        assert_eq!(stats.custom_effect_passes, other.custom_effect_passes);
        assert_eq!(stats.custom_effect_pipelines, other.custom_effect_pipelines);
        assert_eq!(
            stats.custom_effect_bytes_allocated,
            other.custom_effect_bytes_allocated
        );
        Ok((xrack_assets::decode_png(png)?, stats))
    };
    let half = |frame: &xrack_assets::Raster, x: usize| -> Vec<u8> {
        frame
            .rgba
            .chunks_exact(480 * 4)
            .take(240)
            .flat_map(|row| row[x * 4..(x + 240) * 4].iter().copied())
            .collect()
    };
    let mut app = weak.upgrade().ok_or("missing buffer format demo")?;
    let (base, initial) = draw()?;
    assert_eq!(initial.custom_effect_passes, 2);
    assert_eq!(initial.custom_effect_pipelines, 2);
    assert_eq!(initial.custom_effect_target_allocations, 2);
    assert_eq!(
        initial.custom_effect_bytes_allocated,
        FIELD_PIXELS + OUTPUT_PIXELS * 8
    );

    app.set_linear_filter(true);
    dynamic.set("linear-filter", Value::Bool(true))?;
    let (linear, sampled) = draw()?;
    assert_ne!(half(&linear, 0), half(&base, 0));
    assert_eq!(half(&linear, 240), half(&base, 240));
    assert_eq!(
        sampled.custom_effect_pipelines,
        initial.custom_effect_pipelines
    );
    assert_eq!(
        sampled.custom_effect_bytes_allocated,
        initial.custom_effect_bytes_allocated
    );

    let mut previous = linear;
    for mode in ["repeat", "mirror"] {
        app.set_addressing(mode.into());
        dynamic.set("addressing", Value::String(mode.into()))?;
        let (next, stats) = draw()?;
        assert_ne!(half(&next, 0), half(&previous, 0), "{mode}");
        assert_eq!(half(&next, 240), half(&base, 240));
        assert_eq!(
            stats.custom_effect_pipelines,
            initial.custom_effect_pipelines
        );
        assert_eq!(
            stats.custom_effect_bytes_allocated,
            initial.custom_effect_bytes_allocated
        );
        previous = next;
    }

    let mut bytes = initial.custom_effect_bytes_allocated;
    let mut pipelines = initial.custom_effect_pipelines;
    let center = (121 * 480 + 300) * 4;
    let mut ldr_blue = 0;
    for (format, texel_bytes) in [
        ("r16f", 2),
        ("rg8", 2),
        ("rg16f", 4),
        ("rgba8", 4),
        ("rgba16f", 8),
    ] {
        app.set_buffer_format(format.into());
        dynamic.set("buffer-format", Value::String(format.into()))?;
        let (next, stats) = draw()?;
        bytes += FIELD_PIXELS * texel_bytes;
        pipelines += 1;
        assert_eq!(stats.custom_effect_bytes_allocated, bytes, "{format}");
        assert_eq!(stats.custom_effect_pipelines, pipelines, "{format}");
        match format {
            "rg16f" => assert!(next.rgba[center + 1] > 160, "signed channel was lost"),
            "rgba8" => ldr_blue = next.rgba[center + 2],
            "rgba16f" => assert!(
                next.rgba[center + 2] > ldr_blue + 10,
                "HDR channel was clipped"
            ),
            _ => assert!(next.rgba[center + 1] < 70),
        }
    }
    Ok(())
}

#[test]
fn effect_file_and_live_inputs_share_images_and_decode_alpha_correctly() -> TestResult {
    use effect_texture_inputs_ui::EffectTextureInputsDemo;

    const NAME: &str = "examples/puzzle3d/effect_texture_inputs.xr";
    const BACKGROUND: [u8; 4] = [36, 57, 78, 255];
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|path| path.ends_with("models/channel-swatch.png"))
    );
    let dynamic = program.create("EffectTextureInputsDemo")?;
    let app = EffectTextureInputsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 4]; 4], xrack_renderer_femtovg::SceneDrawStats)> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let stats = gpu.scene_draw_stats();
        let other = preview_gpu.scene_draw_stats();
        assert_eq!(stats.scene_image_uploads, other.scene_image_uploads);
        assert_eq!(stats.item_texture_captures, other.item_texture_captures);
        assert_eq!(stats.custom_effect_pipelines, other.custom_effect_pipelines);
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [60, 180, 300, 390].map(|x| {
            let offset = ((120 * raster.size[0] + x) * 4) as usize;
            raster.rgba[offset..offset + 4]
                .try_into()
                .expect("RGBA texel")
        });
        Ok((pixels, stats))
    };
    let near = |actual: [u8; 4], expected: [u8; 4]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{actual:?} vs {expected:?}"
        );
    };

    let mut app = weak.upgrade().ok_or("missing texture input demo")?;
    app.set_show_object(false);
    dynamic.set("show-object", Value::Bool(false))?;
    let (colors, initial) = draw()?;
    near(colors[0], [218, 92, 26, 255]);
    near(colors[1], [238, 162, 90, 255]);
    near(colors[2], [64, 160, 208, 255]);
    assert_eq!(colors[3], BACKGROUND);
    assert_eq!(initial.scene_image_uploads, 1);
    assert_eq!(initial.material_mip_levels_uploaded, 0);
    assert_eq!(initial.item_texture_captures, 1);

    app.set_show_object(true);
    dynamic.set("show-object", Value::Bool(true))?;
    let (shared, after_object) = draw()?;
    assert_eq!(&shared[..3], &colors[..3]);
    assert_ne!(shared[3], BACKGROUND);
    assert_eq!(
        after_object.scene_image_uploads, 1,
        "material and effect share the uploaded image"
    );

    app.set_cool(false);
    dynamic.set("cool", Value::Bool(false))?;
    let (live, updated) = draw()?;
    near(live[2], [208, 128, 96, 255]);
    assert_eq!(updated.scene_image_uploads, 1);
    assert_eq!(
        updated.item_texture_captures,
        initial.item_texture_captures + 1
    );
    assert_eq!(
        updated.item_texture_allocations,
        initial.item_texture_allocations
    );

    app.set_item_mips(true);
    dynamic.set("item-mips", Value::Bool(true))?;
    let (mips, mipmapped) = draw()?;
    near(mips[2], live[2]);
    assert!(mipmapped.item_texture_mip_passes > updated.item_texture_mip_passes);

    app.set_live_enabled(false);
    app.set_file_enabled(false);
    dynamic.set("live-enabled", Value::Bool(false))?;
    dynamic.set("file-enabled", Value::Bool(false))?;
    let (disabled, stats) = draw()?;
    assert_eq!(&disabled[..3], &[BACKGROUND; 3]);
    assert_eq!(disabled[3], shared[3]);
    assert_eq!(
        stats.custom_effect_pipelines,
        initial.custom_effect_pipelines
    );
    assert_eq!(stats.scene_image_uploads, 1);
    Ok(())
}

#[test]
fn authored_material_shader_is_slot_local_and_keeps_alpha_coverage_in_both_backends() -> TestResult
{
    use material_shader_ui::MaterialShaderDemo;

    const NAME: &str = "examples/puzzle3d/material_shader.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for asset in [
        "effects/material_surface.wgsl",
        "effects/material_trim.wgsl",
    ] {
        assert!(inputs.iter().any(|path| path.ends_with(asset)), "{asset}");
    }
    let dynamic = program.create("MaterialShaderDemo")?;
    let app = MaterialShaderDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 4]; 3], [u64; 3])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let work = |gpu: &Renderer| {
            [
                gpu.scene_draw_stats().custom_material_pipeline_builds,
                gpu.scene_draw_stats().mesh_vertex_bytes_uploaded,
                gpu.scene_shadow_passes(),
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [183, 317, 451].map(|x| {
            let i = (162 * 634 + x) * 4;
            raster.rgba[i..i + 4].try_into().expect("RGBA texel")
        });
        Ok((pixels, work(&gpu)))
    };
    let mut app = weak.upgrade().ok_or("missing material shader demo")?;
    let (initial, compiled) = draw()?;
    assert!(initial[0][2] > initial[0][0] + 35);
    assert!(initial[1][0] > initial[1][2] + 35);
    assert_eq!(compiled[0], 2);
    assert_eq!(app.get_body_hit(), "model");
    assert_eq!(dynamic.get("body-hit")?, Value::String("model".into()));

    app.set_intensity(0.18);
    dynamic.set("intensity", Value::Float(0.18))?;
    let (changed, reused) = draw()?;
    assert_ne!(changed[0], initial[0]);
    assert_eq!(&changed[1..], &initial[1..]);
    assert_eq!(
        reused, compiled,
        "shader parameters reuse color pipelines, geometry and shadows"
    );

    app.set_trim_program(false);
    dynamic.set("trim-program", Value::Bool(false))?;
    let (plain_trim, reused) = draw()?;
    assert_eq!(plain_trim[0], changed[0]);
    assert_ne!(plain_trim[1], changed[1]);
    assert_eq!(plain_trim[2], changed[2]);
    assert_eq!(reused, compiled);
    assert_eq!(dynamic.get("body-hit")?, Value::String("model".into()));

    app.set_cutout(true);
    dynamic.set("cutout", Value::Bool(true))?;
    let (cut, _) = draw()?;
    assert_ne!(cut[0], plain_trim[0]);
    assert_eq!(app.get_body_hit(), "wall");
    assert_eq!(dynamic.get("body-hit")?, Value::String("wall".into()));
    Ok(())
}

#[test]
fn material_texture_inputs_share_uploads_and_refresh_live_items_in_both_backends() -> TestResult {
    use material_texture_inputs_ui::MaterialTextureInputsDemo;

    const NAME: &str = "examples/puzzle3d/material_texture_inputs.xr";
    let source = std::fs::read_to_string(NAME)?;
    let preview = xrack_compiler::prepare_preview(&source, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for asset in [
        "effects/material_texture_inputs.wgsl",
        "models/channel-swatch.png",
    ] {
        assert!(inputs.iter().any(|path| path.ends_with(asset)), "{asset}");
    }
    let dynamic = program.create("MaterialTextureInputsDemo")?;
    let app = MaterialTextureInputsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 4]; 3], [u64; 5])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let work = |gpu: &Renderer| {
            let stats = gpu.scene_draw_stats();
            [
                stats.custom_material_pipeline_builds,
                stats.mesh_vertex_bytes_uploaded,
                stats.scene_image_uploads,
                stats.item_texture_captures,
                stats.item_texture_allocations,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [84, 240, 396].map(|x| {
            let i = (120 * 480 + x) * 4;
            raster.rgba[i..i + 4].try_into().expect("RGBA texel")
        });
        Ok((pixels, work(&gpu)))
    };
    let near = |actual: [u8; 4], expected: [u8; 4]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{actual:?} vs {expected:?}",
        );
    };
    let mut app = weak.upgrade().ok_or("missing material texture demo")?;
    let (initial, compiled) = draw()?;
    near(initial[0], [218, 92, 26, 255]);
    near(initial[1], [238, 162, 90, 255]);
    near(initial[2], [64, 160, 208, 255]);
    assert_eq!(compiled[0], 1);
    assert_eq!(compiled[2], 1, "both color spaces share a GPU upload");
    assert_eq!(compiled[3], 1);

    app.set_cool(false);
    dynamic.set("cool", Value::Bool(false))?;
    let (changed, reused) = draw()?;
    assert_eq!(&changed[..2], &initial[..2]);
    near(changed[2], [208, 128, 96, 255]);
    assert_eq!(&reused[..3], &compiled[..3]);
    assert_eq!(reused[3], compiled[3] + 1);
    assert_eq!(reused[4], compiled[4]);

    app.set_file_enabled(false);
    app.set_live_enabled(false);
    dynamic.set("file-enabled", Value::Bool(false))?;
    dynamic.set("live-enabled", Value::Bool(false))?;
    let (disabled, reused) = draw()?;
    assert_eq!(disabled, [disabled[0]; 3]);
    assert_ne!(disabled[0], initial[0]);
    assert_eq!(&reused[..3], &compiled[..3]);
    Ok(())
}

#[test]
fn programmable_vertex_displacement_moves_color_shadows_and_motion_into_view() -> TestResult {
    use vertex_program_ui::VertexProgramDemo;

    const NAME: &str = "examples/puzzle3d/vertex_program.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|path| path.ends_with("effects/vertex_program.wgsl"))
    );
    let dynamic = program.create("VertexProgramDemo")?;
    let app = VertexProgramDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<(xrack_assets::Raster, [u64; 5])> {
        let png = aot.render_animated(&mut gpu)?;
        assert!(png == interpreted.render_animated(&mut preview_gpu)?);
        let work = |gpu: &Renderer| {
            let stats = gpu.scene_draw_stats();
            [
                stats.custom_material_pipeline_builds,
                stats.mesh_vertex_bytes_uploaded,
                gpu.scene_shadow_passes(),
                stats.motion_vector_draws,
                stats.temporal_reprojections,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((xrack_assets::decode_png(png)?, work(&gpu)))
    };
    let pixel = |image: &xrack_assets::Raster, x: usize, y: usize| -> [u8; 3] {
        let start = 4 * (y * image.size[0] as usize + x);
        std::array::from_fn(|channel| image.rgba[start + channel])
    };
    let red = |rgb: [u8; 3]| rgb[0] > rgb[1].saturating_add(100);
    let mut app = weak.upgrade().ok_or("missing vertex program demo")?;
    let (initial, before) = draw()?;
    assert!(red(pixel(&initial, 188, 160)));
    assert!(!red(pixel(&initial, 380, 160)));
    assert!(before[2] > 0 && before[3] > 0, "{before:?}");

    app.set_travel(-2.4);
    dynamic.set("travel", Value::Float(-2.4))?;
    let (moved, after) = draw()?;
    assert!(!red(pixel(&moved, 188, 160)));
    assert!(red(pixel(&moved, 380, 160)));
    let shadow_pixels = (40..280)
        .flat_map(|y| (50..470).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let previous = pixel(&initial, x, y);
            let current = pixel(&moved, x, y);
            !red(previous) && !red(current) && previous[2].abs_diff(current[2]) > 20
        })
        .count();
    assert!(shadow_pixels > 200, "{shadow_pixels} changed shadow pixels");
    assert_eq!(
        &before[..2],
        &after[..2],
        "reuse pipelines and vertex buffers"
    );
    assert!(after[2] > before[2] && after[3] > before[3] && after[4] > before[4]);

    app.set_travel(-60.0);
    dynamic.set("travel", Value::Float(-60.0))?;
    let (clamped, _) = draw()?;
    // The displacement bound is six local units. The clamped mesh centers
    // at x = -2.1, still inside the viewport.
    assert!(red(pixel(&clamped, 92, 160)));
    Ok(())
}

#[test]
fn programmable_material_lighting_preserves_attenuation_shadows_and_pipeline_reuse() -> TestResult {
    use material_lighting_ui::MaterialLightingDemo;

    const NAME: &str = "examples/puzzle3d/material_lighting.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|path| path.ends_with("effects/material_lighting.wgsl")),
        "{inputs:?}",
    );
    let dynamic = program.create("MaterialLightingDemo")?;
    let app = MaterialLightingDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 3])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreter.render(&mut preview_gpu)?);
        let work = |gpu: &Renderer| {
            let stats = gpu.scene_draw_stats();
            [
                stats.custom_material_pipeline_builds,
                stats.mesh_vertex_bytes_uploaded,
                gpu.scene_shadow_passes(),
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [169, 351].map(|x| {
            let start = (135 * 520 + x) * 4;
            std::array::from_fn(|channel| raster.rgba[start + channel])
        });
        Ok((pixels, work(&gpu)))
    };
    let mut app = weak.upgrade().ok_or("missing lighting demo")?;
    let mut set_kind = |kind| -> TestResult {
        app.set_light_kind(kind);
        dynamic.set("light-kind", Value::Int(kind))?;
        Ok(())
    };

    let (directional, initial) = draw()?;
    assert!(
        directional[0][0] > directional[0][1].saturating_add(90),
        "{directional:?}"
    );
    assert_eq!(initial[0], 1);
    set_kind(1)?;
    let (point, reuse) = draw()?;
    assert!(point[0][1] > point[0][2].saturating_add(80), "{point:?}");
    assert_eq!(reuse, initial);
    set_kind(2)?;
    let (spot, reuse) = draw()?;
    assert!(spot[0][2] > spot[0][1].saturating_add(80), "{spot:?}");
    assert_eq!(reuse, initial);
    set_kind(3)?;
    let (ambient, reuse) = draw()?;
    assert!(
        ambient[0][1] > ambient[0][2].saturating_add(50),
        "{ambient:?}"
    );
    assert_eq!(reuse, initial);

    set_kind(1)?;
    app.set_point_range(1.0);
    dynamic.set("point-range", Value::Float(1.0))?;
    let (outside, reuse) = draw()?;
    assert!(
        outside[0][1].saturating_add(120) < point[0][1],
        "{outside:?}"
    );
    assert_eq!(reuse, initial);
    app.set_light_kind(0);
    dynamic.set("light-kind", Value::Int(0))?;
    app.set_specular_gain(1.0);
    dynamic.set("specular-gain", Value::Float(1.0))?;
    let (highlight, reuse) = draw()?;
    assert!(
        highlight[0][2] > directional[0][2].saturating_add(90),
        "{highlight:?}"
    );
    assert_eq!(highlight[1], directional[1]);
    assert_eq!(reuse, initial);

    app.set_shadows(true);
    dynamic.set("shadows", Value::Bool(true))?;
    let (shadowed, shadow_work) = draw()?;
    assert!(
        shadowed[0][0].saturating_add(140) < highlight[0][0],
        "{shadowed:?}"
    );
    assert_eq!(shadowed[1], directional[1]);
    assert!(shadow_work[2] > 0);

    app.set_shadows(false);
    app.set_specular_gain(0.0);
    app.set_amount(0.0);
    dynamic.set("shadows", Value::Bool(false))?;
    dynamic.set("specular-gain", Value::Float(0.0))?;
    dynamic.set("amount", Value::Float(0.0))?;
    let (identity, _) = draw()?;
    app.set_programmable(false);
    dynamic.set("programmable", Value::Bool(false))?;
    let (stock, _) = draw()?;
    assert_eq!(identity, stock);
    Ok(())
}

#[test]
fn scene_effect_surface_inputs_share_capture_and_follow_material_geometry() -> TestResult {
    use effect_surface_ui::EffectSurfaceDemo;

    const NAME: &str = "examples/puzzle3d/effect_surface.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for asset in [
        "effect_surface_data.wgsl",
        "effect_surface_inspect.wgsl",
        "effect_surface_material.wgsl",
        "effect_surface_passthrough.wgsl",
    ] {
        assert!(inputs.iter().any(|path| path.ends_with(asset)), "{asset}");
    }
    let dynamic = program.create("EffectSurfaceDemo")?;
    let app = EffectSurfaceDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 3], [u64; 4])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreter.render(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [169, 260, 351].map(|x| {
            let i = (135 * 520 + x) * 4;
            std::array::from_fn(|c| raster.rgba[i + c])
        });
        let work = |gpu: &Renderer| {
            let stats = gpu.scene_draw_stats();
            [
                stats.custom_effect_surface_passes,
                stats.custom_effect_surface_draws,
                stats.custom_effect_passes,
                stats.mesh_vertex_bytes_uploaded,
            ]
        };
        let stats = work(&gpu);
        assert_eq!(stats, work(&preview_gpu));
        Ok((pixels, stats))
    };
    let near = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{actual:?} vs {expected:?}"
        );
    };

    let (normals, work) = draw()?;
    near(normals[0], [188, 231, 243]);
    near(normals[1], [36, 53, 75]);
    near(normals[2], [229, 188, 245]);
    assert_eq!(
        &work[..3],
        &[1, 2, 2],
        "both shader passes share opaque surface data"
    );
    assert_eq!(
        draw()?.1,
        work,
        "unchanged viewport retains the final result"
    );

    let mut app = weak.upgrade().ok_or("missing effect surface demo")?;
    app.set_glass(false);
    dynamic.set("glass", Value::Bool(false))?;
    let (without_glass, _) = draw()?;
    assert_eq!(normals, without_glass);
    app.set_mode(1);
    dynamic.set("mode", Value::Int(1))?;
    let (roughness, work1) = draw()?;
    near(roughness[0], [124, 124, 0]);
    near(roughness[2], [226, 226, 0]);
    assert_eq!(
        work[3], work1[3],
        "switching the diagnostic reads the original mesh streams"
    );

    app.set_mode(2);
    dynamic.set("mode", Value::Int(2))?;
    let (world, _) = draw()?;
    near(world[0], [146, 187, 211]);
    app.set_use_perspective(true);
    dynamic.set("use-perspective", Value::Bool(true))?;
    let (perspective, _) = draw()?;
    assert_ne!(
        perspective[0][0], world[0][0],
        "camera changes the sampled world X"
    );
    assert!(
        perspective[0][2].abs_diff(world[0][2]) <= 2,
        "world Z is projection independent"
    );
    app.set_lift(0.2);
    dynamic.set("lift", Value::Float(0.2))?;
    let (lifted, active) = draw()?;
    assert!(lifted[0][2].abs_diff(188) <= 2);

    app.set_inspect_surfaces(false);
    dynamic.set("inspect-surfaces", Value::Bool(false))?;
    let (color, disabled) = draw()?;
    assert_ne!(color[0], lifted[0]);
    assert_eq!(
        &disabled[..2],
        &active[..2],
        "unreachable helper does not request a capture"
    );
    assert_eq!(disabled[2], active[2] + 1);
    Ok(())
}

#[test]
fn named_scene_captures_share_targets_and_invalidate_only_on_scene_input_changes() -> TestResult {
    use scene_capture_ui::SceneCaptureDemo;

    const NAME: &str = "examples/puzzle3d/scene_capture.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|path| path.ends_with("scene_capture_display.wgsl"))
    );
    let dynamic = program.create("SceneCaptureDemo")?;
    let app = SceneCaptureDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 3], [u64; 4])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreter.render(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [(140, 70), (420, 70), (280, 210)].map(|(x, y)| {
            let start = (y * 560 + x) * 4;
            std::array::from_fn(|channel| raster.rgba[start + channel])
        });
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_reuses,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((pixels, work(&gpu)))
    };

    let (near, first) = draw()?;
    assert_eq!(near, [[227, 69, 64], [227, 69, 64], [81, 188, 131]]);
    assert_eq!(
        &first[..3],
        &[1, 1, 280 * 140 * 8],
        "both aliases share one HDR target"
    );
    assert_eq!(draw()?.1, first, "retained viewport");
    let mut app = weak.upgrade().ok_or("missing scene capture demo")?;

    app.set_exposure(0.4);
    dynamic.set("exposure", Value::Float(0.4))?;
    let (dim, reused) = draw()?;
    assert!(dim[0][0] < near[0][0]);
    assert_eq!(&dim[1..], &near[1..]);
    assert_eq!(reused, [first[0], first[1] + 2, first[2], first[3]]);

    app.set_exposure(1.0);
    app.set_feed_camera("far-eye".into());
    dynamic.set("exposure", Value::Float(1.0))?;
    dynamic.set("feed-camera", Value::String("far-eye".into()))?;
    let (far, changed) = draw()?;
    assert_eq!(far, [[57, 174, 240], [57, 174, 240], near[2]]);
    assert_eq!(changed[0], first[0] + 1);
    assert_eq!(&changed[2..], &first[2..]);

    app.set_feed_camera("near-eye".into());
    app.set_feed_layers(4);
    dynamic.set("feed-camera", Value::String("near-eye".into()))?;
    dynamic.set("feed-layers", Value::Int(4))?;
    let (other_layer, changed) = draw()?;
    assert_eq!(other_layer, [[255, 188, 66], [255, 188, 66], near[2]]);
    assert_eq!(changed[0], first[0] + 2);

    app.set_feed_layers(2);
    app.set_near_color(xrack_runtime::Color(0xb048deff));
    dynamic.set("feed-layers", Value::Int(2))?;
    dynamic.set("near-color", Value::Color(xrack_runtime::Color(0xb048deff)))?;
    let (new_color, changed) = draw()?;
    assert_eq!(new_color, [[176, 72, 222], [176, 72, 222], near[2]]);
    assert_eq!(changed[0], first[0] + 3);
    assert_eq!(changed[2], first[2]);

    app.set_feed_scale(0.75);
    dynamic.set("feed-scale", Value::Float(0.75))?;
    let (scaled, changed) = draw()?;
    assert_eq!(scaled, new_color);
    assert_eq!(changed[0], first[0] + 4);
    assert_eq!(changed[2], first[2] + 420 * 210 * 8);

    app.set_feed_camera("missing".into());
    dynamic.set("feed-camera", Value::String("missing".into()))?;
    let (missing, missing_work) = draw()?;
    assert_eq!(missing, [[66, 72, 82], [66, 72, 82], near[2]]);
    assert_eq!(missing_work, changed);

    app.set_feed_camera("near-eye".into());
    app.set_display_feeds(false);
    dynamic.set("feed-camera", Value::String("near-eye".into()))?;
    dynamic.set("display-feeds", Value::Bool(false))?;
    let (off, unrequested) = draw()?;
    assert_eq!(off, [[36, 58, 82], [36, 58, 82], near[2]]);
    assert_eq!(unrequested, changed);
    Ok(())
}

#[test]
fn capture_backed_materials_share_sources_resolve_dependencies_and_break_cycles() -> TestResult {
    use capture_material_ui::CaptureMaterialDemo;

    const NAME: &str = "examples/puzzle3d/capture_material.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, resources) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for asset in [
        "capture_material_display.wgsl",
        "capture_material_inset.wgsl",
    ] {
        assert!(resources.iter().any(|path| path.ends_with(asset)));
    }
    let dynamic = program.create("CaptureMaterialDemo")?;
    let app = CaptureMaterialDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 3], [u64; 5])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreter.render(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [(163, 140), (357, 140), (260, 36)].map(|(x, y)| {
            let i = (y * 520 + x) * 4;
            std::array::from_fn(|c| raster.rgba[i + c])
        });
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.scene_capture_material_bindings,
                stats.scene_capture_feedback_edges,
                stats.mesh_vertex_bytes_uploaded,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((pixels, work(&gpu)))
    };

    let (base, first) = draw()?;
    assert_eq!(base, [[220, 78, 66]; 3]);
    assert_eq!(first[0], 2, "inset and direct material share the producer");
    assert_eq!(first[1], 2 * 260 * 140 * 8);
    assert!(first[2] > 0);
    assert_eq!(first[3], 0);
    assert_eq!(draw()?.1, first, "unchanged viewport");

    let mut app = weak.upgrade().ok_or("missing capture material demo")?;
    app.set_source_layer(8);
    dynamic.set("source-layer", Value::Int(8))?;
    let (alternate, changed) = draw()?;
    assert_eq!(alternate, [[70, 142, 210]; 3]);
    assert_eq!(
        changed,
        [first[0] + 2, first[1], first[2], first[3], first[4]]
    );

    app.set_inset(false);
    dynamic.set("inset", Value::Bool(false))?;
    let (material_only, reused) = draw()?;
    assert_eq!(material_only, [alternate[0], alternate[1], [35, 56, 79]]);
    assert_eq!(
        reused, changed,
        "material demand keeps the same capture targets"
    );

    app.set_source_layer(4);
    app.set_feedback(true);
    app.set_inset(true);
    dynamic.set("source-layer", Value::Int(4))?;
    dynamic.set("feedback", Value::Bool(true))?;
    dynamic.set("inset", Value::Bool(true))?;
    let (cycle, fallback) = draw()?;
    assert_eq!(cycle, [[196, 44, 196]; 3]);
    assert_eq!(
        fallback[3], 2,
        "both edges in the cycle use missing-texture values"
    );
    assert_eq!(fallback[0], changed[0] + 2);
    assert_eq!(fallback[1], changed[1]);

    app.set_inset(false);
    app.set_direct_visible(false);
    dynamic.set("inset", Value::Bool(false))?;
    dynamic.set("direct-visible", Value::Bool(false))?;
    let (single, pruned) = draw()?;
    assert_eq!(single, [cycle[0], [35, 56, 79], [35, 56, 79]]);
    assert_eq!(
        pruned[0],
        fallback[0] + 1,
        "a disconnected producer is not rendered"
    );

    app.set_inset(true);
    app.set_direct_visible(true);
    dynamic.set("inset", Value::Bool(true))?;
    dynamic.set("direct-visible", Value::Bool(true))?;
    app.set_feedback(false);
    dynamic.set("feedback", Value::Bool(false))?;
    let (restored, last) = draw()?;
    assert_eq!(restored, base);
    assert_eq!(last[0], pruned[0] + 2);
    assert_eq!(last[1], first[1]);
    assert_eq!(last[3], pruned[3]);
    Ok(())
}

#[test]
fn reflection_probes_sample_named_captures_and_keep_sliced_faces_coherent() -> TestResult {
    use probe_capture_feed_ui::ProbeCaptureFeedDemo;

    const NAME: &str = "examples/puzzle3d/probe_capture_feed.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        inputs
            .iter()
            .any(|path| path.ends_with("capture_material_display.wgsl"))
    );
    let dynamic = program.create("ProbeCaptureFeedDemo")?;
    let app = ProbeCaptureFeedDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 5])> {
        let png = aot.render_animated(&mut gpu)?;
        assert!(png == interpreted.render_animated(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [(266, 135), (452, 47)].map(|(x, y)| {
            let start = (y * 532 + x) * 4;
            std::array::from_fn(|c| raster.rgba[start + c])
        });
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.reflection_capture_faces,
                stats.reflection_cubemap_updates,
                stats.reflection_prefilter_passes,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((pixels, work(&gpu)))
    };
    let (warm, first) = draw()?;
    assert!(warm[0][0] > warm[0][2].saturating_add(100), "{warm:?}");
    assert_eq!(&first[..4], &[1, 266 * 135 * 8, 6, 1]);
    assert_eq!(
        draw()?.1,
        first,
        "unchanged inputs reuse color and cubemap textures"
    );
    let mut app = weak.upgrade().ok_or("missing probe capture feed demo")?;

    // This changes only the capture descriptor. The probe projection and
    // source scene stay stable, so the feed revision must invalidate it.
    app.set_source_layer(4);
    dynamic.set("source-layer", Value::Int(4))?;
    let (cool, changed) = draw()?;
    assert!(cool[0][2] > cool[0][0].saturating_add(100), "{cool:?}");
    assert_eq!(&changed[..4], &[2, first[1], 12, 2]);

    app.set_inset(true);
    dynamic.set("inset", Value::Bool(true))?;
    let (inset, reused) = draw()?;
    assert_eq!(inset, [cool[0], [71, 142, 216]]);
    assert_eq!(
        reused, changed,
        "direct effect and probe share the same feed"
    );

    app.set_refresh("first-frame".into());
    dynamic.set("refresh", Value::String("first-frame".into()))?;
    let (fixed, locked) = draw()?;
    assert_eq!(fixed, inset);
    assert_eq!(&locked[..4], &[2, first[1], 18, 3]);
    app.set_source_layer(2);
    app.set_inset(false);
    dynamic.set("source-layer", Value::Int(2))?;
    dynamic.set("inset", Value::Bool(false))?;
    let (frozen, skipped) = draw()?;
    assert_eq!(frozen[0], cool[0]);
    assert_eq!(
        skipped, locked,
        "the completed first-frame probe no longer needs its private feed"
    );

    app.set_revision(1);
    dynamic.set("revision", Value::Int(1))?;
    let (refresh, updated) = draw()?;
    assert_eq!(refresh[0], warm[0]);
    assert_eq!(&updated[..4], &[3, first[1], 24, 4]);

    app.set_source_layer(4);
    app.set_refresh("on-change".into());
    app.set_slicing("individual-faces".into());
    dynamic.set("source-layer", Value::Int(4))?;
    dynamic.set("refresh", Value::String("on-change".into()))?;
    dynamic.set("slicing", Value::String("individual-faces".into()))?;
    let (previous, sliced) = draw()?;
    assert_eq!(
        previous[0], warm[0],
        "old cube is published while filtering"
    );
    assert_eq!(&sliced[..4], &[4, first[1], 25, 4]);
    assert_eq!(
        sliced[4], updated[4],
        "one base face is recorded before filtering"
    );
    app.set_source_layer(2);
    dynamic.set("source-layer", Value::Int(2))?;
    let (while_filtering, dirty) = draw()?;
    assert_eq!(while_filtering[0], warm[0]);
    assert_eq!(
        &dirty[..4],
        &[sliced[0] + 1, sliced[1] * 2, sliced[2] + 1, sliced[3]]
    );
    let mut complete = false;
    for step in 1..=10 {
        let (frame, stats) = draw()?;
        assert_eq!(
            &stats[..2],
            &dirty[..2],
            "unchanged live inputs reuse the replacement texture"
        );
        assert_eq!(stats[2], (dirty[2] + step).min(sliced[2] + 5));
        if stats[3] > sliced[3] {
            assert_eq!(frame[0], cool[0]);
            complete = true;
            break;
        }
        assert_eq!(frame[0], warm[0]);
    }
    assert!(complete);

    let (still_cool, next) = draw()?;
    assert_eq!(still_cool[0], cool[0]);
    assert_eq!(
        &next[..4],
        &[dirty[0], dirty[1], sliced[2] + 6, sliced[3] + 1]
    );
    let mut refreshed = false;
    for step in 1..=11 {
        let (frame, stats) = draw()?;
        assert_eq!(&stats[..2], &next[..2]);
        assert_eq!(stats[2], (next[2] + step).min(next[2] + 5));
        if stats[3] > next[3] {
            assert_eq!(frame[0], warm[0]);
            refreshed = true;
            break;
        }
        assert_eq!(frame[0], cool[0]);
    }
    assert!(
        refreshed,
        "the newer feed is published after the first complete cube"
    );
    Ok(())
}

#[test]
fn sliced_probes_pin_only_consumed_mrt_attachments_through_the_last_base_face() -> TestResult {
    use probe_capture_feed_ui::ProbeCaptureFeedDemo;

    const NAME: &str = "examples/puzzle3d/probe_capture_feed.xr";
    const PIXELS: u64 = 266 * 135;
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ProbeCaptureFeedDemo")?;
    let mut app = ProbeCaptureFeedDemo::new();
    app.set_inset(true);
    app.set_inset_depth(true);
    app.set_combine_feed(true);
    app.set_slicing("individual-faces".into());
    for name in ["inset", "inset-depth", "combine-feed"] {
        dynamic.set(name, Value::Bool(true))?;
    }
    dynamic.set("slicing", Value::String("individual-faces".into()))?;
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 6])> {
        let png = aot.render_animated(&mut gpu)?;
        assert!(png == interpreted.render_animated(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [(266, 135), (452, 47)].map(|(x, y)| {
            let start = (y * 532 + x) * 4;
            std::array::from_fn(|c| raster.rgba[start + c])
        });
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_mrt_passes,
                stats.scene_capture_bytes_allocated,
                stats.reflection_capture_faces,
                stats.reflection_cubemap_updates,
                stats.reflection_prefilter_passes,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((pixels, work(&gpu)))
    };

    let (warm_inset, first) = draw()?;
    assert_eq!(first, [1, 1, PIXELS * 12, 1, 0, 0]);
    let mut app = weak.upgrade().ok_or("missing probe MRT fixture")?;
    app.set_source_layer(4);
    dynamic.set("source-layer", Value::Int(4))?;
    let (cool_inset, changed) = draw()?;
    assert_eq!(changed, [2, 2, PIXELS * 20, 2, 0, 0]);
    assert!(cool_inset[1][2] > warm_inset[1][2] + 70);

    for face in 3..=6 {
        let (_, work) = draw()?;
        assert_eq!(work, [2, 2, PIXELS * 20, face, 0, 0]);
    }
    let mut warm = None;
    for _ in 0..6 {
        let (image, work) = draw()?;
        assert_eq!(&work[..4], &[2, 2, PIXELS * 20, 6]);
        if work[4] == 1 {
            assert!(image[0][0] > image[0][2].saturating_add(100), "{image:?}");
            warm = Some(image[0]);
        }
    }
    let warm = warm.ok_or("warm cube was not published")?;

    for face in 7..=12 {
        let (image, work) = draw()?;
        assert_eq!(image[0], warm);
        assert_eq!(&work[..5], &[2, 2, PIXELS * 20, face, 1]);
    }
    app.set_source_layer(2);
    dynamic.set("source-layer", Value::Int(2))?;
    let (live, filtering) = draw()?;
    assert_eq!(live[0], warm);
    assert_eq!(&filtering[..5], &[3, 3, PIXELS * 20, 12, 1]);
    assert_eq!(
        live[1], warm_inset[1],
        "the live inset updates without a new texture"
    );
    let mut cool = None;
    for _ in 0..5 {
        let (image, work) = draw()?;
        assert_eq!(&work[..4], &[3, 3, PIXELS * 20, 12]);
        if work[4] == 2 {
            assert!(image[0][2] > image[0][0].saturating_add(100), "{image:?}");
            cool = Some(image[0]);
        }
    }
    assert!(cool.is_some(), "the filtered cube kept its original source");

    app.set_slicing("none".into());
    app.set_reflected_inset(true);
    app.set_combine_feed(false);
    dynamic.set("slicing", Value::String("none".into()))?;
    dynamic.set("reflected-inset", Value::Bool(true))?;
    dynamic.set("combine-feed", Value::Bool(false))?;
    let (split, split_stats) = draw()?;
    assert!(split[1][0] > split[1][2].saturating_add(70), "{split:?}");
    app.set_combine_feed(true);
    dynamic.set("combine-feed", Value::Bool(true))?;
    let (combined, combined_stats) = draw()?;
    assert_eq!(
        combined, split,
        "the joint viewport capture still samples its local probe"
    );
    assert_eq!(combined_stats[0], split_stats[0] + 1);
    assert_eq!(combined_stats[1], split_stats[1] + 1);
    assert_eq!(&combined_stats[3..], &split_stats[3..]);
    app.set_debug_view(true);
    dynamic.set("debug-view", Value::Bool(true))?;
    assert_eq!(
        draw()?.1,
        combined_stats,
        "probe guides do not redraw named inputs"
    );
    Ok(())
}

#[test]
fn sliced_probes_preserve_live_item_inputs_without_freezing_the_viewport() -> TestResult {
    use probe_capture_feed_ui::ProbeCaptureFeedDemo;

    const NAME: &str = "examples/puzzle3d/probe_capture_feed.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let warmer = |pixel: [u8; 3]| pixel[0] > pixel[2].saturating_add(100);
    let cooler = |pixel: [u8; 3]| pixel[2] > pixel[0].saturating_add(100);
    for mipmaps in [false, true] {
        let dynamic = program.create("ProbeCaptureFeedDemo")?;
        let mut app = ProbeCaptureFeedDemo::new();
        app.set_item_source(true);
        app.set_inset(true);
        app.set_item_mipmaps(mipmaps);
        app.set_item_base(mipmaps);
        app.set_slicing("individual-faces".into());
        for (name, value) in [
            ("item-source", true),
            ("inset", true),
            ("item-mipmaps", mipmaps),
            ("item-base", mipmaps),
        ] {
            dynamic.set(name, Value::Bool(value))?;
        }
        dynamic.set("slicing", Value::String("individual-faces".into()))?;
        let weak = app.as_weak();
        let mut aot = PreviewScene::new(Box::new(app));
        let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
        let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
        let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 6])> {
            let png = aot.render_animated(&mut gpu)?;
            assert!(png == interpreted.render_animated(&mut preview_gpu)?);
            let raster = xrack_assets::decode_png(png)?;
            let pixels = [(266, 135), (452, 47)].map(|(x, y)| {
                let start = (y * 532 + x) * 4;
                std::array::from_fn(|c| raster.rgba[start + c])
            });
            let work = |renderer: &Renderer| {
                let stats = renderer.scene_draw_stats();
                assert_eq!(stats.scene_capture_passes, 0);
                [
                    stats.reflection_capture_faces,
                    stats.reflection_cubemap_updates,
                    stats.item_texture_allocations,
                    stats.item_texture_mip_allocations,
                    stats.item_texture_captures,
                    stats.mesh_vertex_bytes_uploaded,
                ]
            };
            assert_eq!(work(&gpu), work(&preview_gpu));
            Ok((pixels, work(&gpu)))
        };
        let (initial, first) = draw()?;
        assert_eq!(&first[..5], &[1, 0, 2, u64::from(mipmaps), 2]);
        assert!(warmer(initial[1]));

        let mut app = weak.upgrade().ok_or("missing probe item texture demo")?;
        app.set_badge_lit(true);
        dynamic.set("badge-lit", Value::Bool(true))?;
        let (badge, next) = draw()?;
        assert_eq!(badge[1], initial[1]);
        assert_eq!(next, [2, 0, first[2], first[3], first[4] + 1, first[5]]);

        app.set_source_layer(4);
        dynamic.set("source-layer", Value::Int(4))?;
        let (live, changed) = draw()?;
        assert!(cooler(live[1]));
        assert_eq!(
            changed,
            [
                3,
                0,
                next[2] + u64::from(!mipmaps),
                next[3] + u64::from(mipmaps),
                next[4] + 1,
                next[5]
            ],
            "replace only the image that the pending probe actually samples"
        );
        let mut published = [0; 3];
        let mut warmed = changed;
        for _ in 0..9 {
            let (image, work) = draw()?;
            assert_eq!(&work[2..5], &changed[2..5]);
            if work[1] == 1 {
                published = image[0];
                // A later face can be the first view of a reflected mesh.
                warmed = work;
                assert_eq!(work[0], 6);
            }
        }
        assert!(warmer(published), "{published:?}");

        // When only the roughness mips are left, the producer can redraw
        // the current image in place even though the probe hasn't published.
        for face in 7..=12 {
            let (image, work) = draw()?;
            assert_eq!(image[0], published);
            assert_eq!(&work[..2], &[face, 1]);
            assert_eq!(&work[2..], &warmed[2..]);
        }
        app.set_source_layer(2);
        dynamic.set("source-layer", Value::Int(2))?;
        let (image, filtering) = draw()?;
        assert!(warmer(image[1]));
        assert_eq!(
            filtering,
            [12, 1, warmed[2], warmed[3], warmed[4] + 1, warmed[5]]
        );
        let mut cool_published = false;
        for _ in 0..5 {
            let (image, work) = draw()?;
            assert_eq!(&work[2..], &filtering[2..]);
            if work[1] == 2 {
                assert!(cooler(image[0]), "{image:?}");
                cool_published = true;
            }
        }
        assert!(cool_published);
        for _ in 0..12 {
            draw()?;
        }
        let (stable, settled) = draw()?;
        assert_eq!(&settled[..2], &[18, 3]);
        assert!(warmer(stable[0]));

        app.set_badge_lit(false);
        dynamic.set("badge-lit", Value::Bool(false))?;
        let (unrelated, work) = draw()?;
        assert_eq!(unrelated, stable);
        assert_eq!(
            work,
            [18, 3, settled[2], settled[3], settled[4] + 1, settled[5]]
        );
    }
    Ok(())
}

#[test]
fn reflection_work_budgets_preserve_snapshots_and_share_work_between_probes() -> TestResult {
    use probe_capture_feed_ui::ProbeCaptureFeedDemo;

    const NAME: &str = "examples/puzzle3d/probe_capture_feed.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Work {
        faces: u64,
        filters: u64,
        publications: u64,
        item_allocations: u64,
    }
    for two in [false, true] {
        let dynamic = program.create("ProbeCaptureFeedDemo")?;
        let mut app = ProbeCaptureFeedDemo::new();
        let (faces, filters, refresh) = if two {
            (1, 30, "every-frame")
        } else {
            (2, 3, "on-change")
        };
        app.set_two_probes(two);
        app.set_item_source(!two);
        app.set_inset(!two);
        app.set_face_budget(faces);
        app.set_filter_budget(filters);
        app.set_refresh(refresh.into());
        for (name, value) in [("two-probes", two), ("item-source", !two), ("inset", !two)] {
            dynamic.set(name, Value::Bool(value))?;
        }
        dynamic.set("face-budget", Value::Int(faces))?;
        dynamic.set("filter-budget", Value::Int(filters))?;
        dynamic.set("refresh", Value::String(refresh.into()))?;
        let weak = app.as_weak();
        let mut aot = PreviewScene::new(Box::new(app));
        let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
        let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
        let mut draw = || -> TestResult<([[u8; 3]; 2], Work)> {
            let png = aot.render_animated(&mut gpu)?;
            assert!(png == interpreted.render_animated(&mut preview_gpu)?);
            let raster = xrack_assets::decode_png(png)?;
            let pixels = [(266, 135), (452, 47)].map(|(x, y)| {
                let start = (y * 532 + x) * 4;
                std::array::from_fn(|c| raster.rgba[start + c])
            });
            let work = |renderer: &Renderer| {
                let stats = renderer.scene_draw_stats();
                Work {
                    faces: stats.reflection_capture_faces,
                    filters: stats.reflection_prefilter_passes,
                    publications: stats.reflection_cubemap_updates,
                    item_allocations: stats.item_texture_allocations,
                }
            };
            assert_eq!(work(&gpu), work(&preview_gpu));
            Ok((pixels, work(&gpu)))
        };
        let mut app = weak.upgrade().ok_or("missing reflection budget fixture")?;
        if two {
            for frame in 1..=12_u64 {
                let (_, work) = draw()?;
                assert_eq!(
                    work,
                    Work {
                        faces: frame,
                        filters: frame.saturating_sub(10) * 30,
                        publications: frame.saturating_sub(10),
                        item_allocations: 0,
                    },
                    "the probes alternate base faces before either can publish"
                );
            }
            app.set_filter_budget(3);
            dynamic.set("filter-budget", Value::Int(3))?;
            let mut last = Work {
                faces: 12,
                filters: 60,
                publications: 2,
                item_allocations: 0,
            };
            for _ in 0..36 {
                let (_, work) = draw()?;
                assert!(work.faces - last.faces <= 1, "{last:?} -> {work:?}");
                assert!(work.filters - last.filters <= 3, "{last:?} -> {work:?}");
                last = work;
            }
            assert!(last.publications >= 4, "{last:?}");
        } else {
            let (_, first) = draw()?;
            assert_eq!(
                first,
                Work {
                    faces: 2,
                    filters: 0,
                    publications: 0,
                    item_allocations: 2
                }
            );
            app.set_source_layer(4);
            dynamic.set("source-layer", Value::Int(4))?;
            let (live, next) = draw()?;
            assert!(live[1][2] > live[1][0].saturating_add(100));
            assert_eq!(
                next,
                Work {
                    faces: 4,
                    filters: 0,
                    publications: 0,
                    item_allocations: 3
                }
            );
            for filters in [3, 6, 9, 12] {
                let (_, work) = draw()?;
                assert_eq!(
                    work,
                    Work {
                        faces: 6,
                        filters,
                        publications: 0,
                        item_allocations: 3
                    }
                );
            }
            app.set_filter_budget(0);
            dynamic.set("filter-budget", Value::Int(0))?;
            let (warm, work) = draw()?;
            assert_eq!(
                work,
                Work {
                    faces: 6,
                    filters: 30,
                    publications: 1,
                    item_allocations: 3
                }
            );
            assert!(warm[0][0] > warm[0][2].saturating_add(100), "{warm:?}");
            assert_eq!(warm[1], live[1]);

            app.set_filter_budget(3);
            dynamic.set("filter-budget", Value::Int(3))?;
            for _ in 0..11 {
                let (previous, work) = draw()?;
                assert_eq!(work.publications, 1);
                assert_eq!(previous[0], warm[0]);
            }
            let (cool, work) = draw()?;
            assert_eq!(
                work,
                Work {
                    faces: 12,
                    filters: 60,
                    publications: 2,
                    item_allocations: 3
                }
            );
            assert!(cool[0][2] > cool[0][0].saturating_add(100), "{cool:?}");
        }
    }
    Ok(())
}

#[test]
fn reflection_texel_budgets_resume_partial_faces_and_produce_the_same_cube() -> TestResult {
    use probe_capture_feed_ui::ProbeCaptureFeedDemo;

    const NAME: &str = "examples/puzzle3d/probe_capture_feed.xr";
    const FILTER_TEXELS: u64 = 6 * (32 * 32 + 16 * 16 + 8 * 8 + 4 * 4 + 2 * 2 + 1);
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("ProbeCaptureFeedDemo")?;
    let mut app = ProbeCaptureFeedDemo::new();
    app.set_probe_size(64);
    app.set_inset(true);
    dynamic.set("probe-size", Value::Int(64))?;
    dynamic.set("inset", Value::Bool(true))?;
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Work {
        faces: u64,
        passes: u64,
        texels: u64,
        publications: u64,
    }
    let mut draw = || -> TestResult<([u8; 3], Work, Vec<u8>)> {
        let png = aot.render_animated(&mut gpu)?;
        assert!(png == interpreted.render_animated(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let start = (135 * 532 + 266) * 4;
        let pixel = std::array::from_fn(|channel| raster.rgba[start + channel]);
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            Work {
                faces: stats.reflection_capture_faces,
                passes: stats.reflection_prefilter_passes,
                texels: stats.reflection_prefilter_texels,
                publications: stats.reflection_cubemap_updates,
            }
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((pixel, work(&gpu), png.to_vec()))
    };
    let (warm, mut previous, _) = draw()?;
    assert!(warm[0] > warm[2].saturating_add(100), "{warm:?}");
    assert_eq!(
        previous,
        Work {
            faces: 6,
            passes: 36,
            texels: FILTER_TEXELS,
            publications: 1
        }
    );
    let mut app = weak.upgrade().ok_or("missing probe texel budget fixture")?;
    app.set_source_layer(4);
    dynamic.set("source-layer", Value::Int(4))?;
    for (pass_cap, texel_cap, passes, texels) in [
        (0, 13, 1, 13),
        (0, 13, 1, 13),
        (0, 13, 1, 13),
        (1, 1280, 1, 985),
        (0, 1301, 2, 1301),
    ] {
        app.set_filter_budget(pass_cap);
        app.set_texel_budget(texel_cap);
        dynamic.set("filter-budget", Value::Int(pass_cap))?;
        dynamic.set("texel-budget", Value::Int(texel_cap))?;
        let (visible, work, _) = draw()?;
        assert_eq!(visible, warm, "an unfinished mip face is never published");
        assert_eq!(
            work,
            Work {
                faces: 12,
                passes: previous.passes + passes,
                texels: previous.texels + texels,
                publications: 1,
            }
        );
        previous = work;
    }
    app.set_texel_budget(0);
    dynamic.set("texel-budget", Value::Int(0))?;
    let (cool, completed, filtered) = draw()?;
    assert!(cool[2] > cool[0].saturating_add(100), "{cool:?}");
    assert_eq!(
        completed,
        Work {
            faces: 12,
            passes: 76,
            texels: FILTER_TEXELS * 2,
            publications: 2
        }
    );

    let mut unlimited = ProbeCaptureFeedDemo::new();
    unlimited.set_probe_size(64);
    unlimited.set_source_layer(4);
    unlimited.set_inset(true);
    let mut reference = PreviewScene::new(Box::new(unlimited));
    assert!(
        filtered == reference.render_animated(&mut renderer()?)?,
        "splitting a face preserves its fragment coordinates and previous rows"
    );

    app.set_probe_size(32);
    app.set_two_probes(true);
    app.set_face_budget(1);
    app.set_filter_budget(2);
    app.set_texel_budget(139);
    for (name, value) in [
        ("probe-size", 32),
        ("face-budget", 1),
        ("filter-budget", 2),
        ("texel-budget", 139),
    ] {
        dynamic.set(name, Value::Int(value))?;
    }
    dynamic.set("two-probes", Value::Bool(true))?;
    let mut previous = completed;
    for _ in 0..60 {
        let (_, work, _) = draw()?;
        assert!(work.faces - previous.faces <= 1, "{previous:?} -> {work:?}");
        assert!(
            work.passes - previous.passes <= 2,
            "{previous:?} -> {work:?}"
        );
        assert!(
            work.texels - previous.texels <= 139,
            "{previous:?} -> {work:?}"
        );
        if work.publications == completed.publications + 2 {
            assert_eq!(work.faces, completed.faces + 12);
            assert_eq!(work.texels, completed.texels + 2 * 2046);
            return Ok(());
        }
        previous = work;
    }
    Err("both probes should publish within the shared texel budget".into())
}

#[test]
fn reflection_probe_guides_respect_depth_and_reuse_reflected_lighting() -> TestResult {
    use live_reflections_ui::LiveReflectionsDemo;

    const NAME: &str = "examples/puzzle3d/live_reflections.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("LiveReflectionsDemo")?;
    let mut app = LiveReflectionsDemo::new();
    app.set_probe_x(1.9);
    dynamic.set("probe-x", Value::Float(1.9))?;
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<(Vec<u8>, [usize; 2], [u64; 4])> {
        let png = aot.render_animated(&mut gpu)?;
        assert!(png == interpreted.render_animated(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let scene = raster.rgba[..532 * 270 * 4].to_vec();
        let mut guides = [0; 2];
        for &[red, green, blue, _] in scene.as_chunks::<4>().0 {
            guides[0] += usize::from(red < 55 && green > 225 && blue > 246);
            guides[1] += usize::from(red > 248 && (173..=212).contains(&green) && blue < 65);
        }
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.reflection_cubemap_updates,
                stats.reflection_debug_draws,
                stats.multisample_resolves,
                stats.supersample_resolves,
            ]
        };
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((scene, guides, work(&gpu)))
    };

    let (plain, colors, first) = draw()?;
    assert_eq!(colors, [0, 0]);
    let mut app = weak.upgrade().ok_or("missing probe guide fixture")?;
    app.set_debug_view(true);
    dynamic.set("debug-view", Value::Bool(true))?;
    let (with_guides, colors, enabled) = draw()?;
    assert!(colors[0] > 50 && colors[1] > 1, "{colors:?}");
    assert_eq!(enabled, [first[0], first[1] + 1, first[2], first[3]]);
    app.set_debug_view(false);
    dynamic.set("debug-view", Value::Bool(false))?;
    let (restored, hidden, disabled) = draw()?;
    assert!(
        restored == plain,
        "hiding guides restores the original viewport"
    );
    assert_eq!((hidden, disabled), ([0, 0], enabled));

    app.set_debug_view(true);
    app.set_probe_box_x(0.8);
    dynamic.set("debug-view", Value::Bool(true))?;
    dynamic.set("probe-box-x", Value::Float(0.8))?;
    let (shifted, shifted_colors, shifted_work) = draw()?;
    assert!(shifted != with_guides && shifted_colors[0] > 50);
    assert_eq!(
        shifted_colors[1], colors[1],
        "box offset keeps the capture marker in place"
    );
    assert_eq!(shifted_work[0], enabled[0]);

    app.set_probe_x(0.0);
    dynamic.set("probe-x", Value::Float(0.0))?;
    let (_, occluded, first_depth) = draw()?;
    assert!(occluded[0] > 50);
    assert_eq!(occluded[1], 0, "the sphere obscures its probe center");
    app.set_antialias(true);
    dynamic.set("antialias", Value::Bool(true))?;
    let (_, multisampled, aa) = draw()?;
    assert!(multisampled[0] > 50);
    assert_eq!(multisampled[1], 0);
    assert!(aa[2] > first_depth[2]);
    app.set_guide_resolution_scale(1.5);
    dynamic.set("guide-resolution-scale", Value::Float(1.5))?;
    let (_, supersampled, ssaa) = draw()?;
    assert!(supersampled[0] > 50);
    assert_eq!(supersampled[1], 0);
    assert!(ssaa[3] > aa[3]);
    Ok(())
}

#[test]
fn named_depth_and_normal_attachments_share_geometry_and_sample_in_materials_and_effects()
-> TestResult {
    use capture_attachments_ui::CaptureAttachmentsDemo;

    const NAME: &str = "examples/puzzle3d/capture_attachments.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for asset in [
        "capture_attachment_material.wgsl",
        "capture_attachment_strip.wgsl",
    ] {
        assert!(inputs.iter().any(|path| path.ends_with(asset)));
    }
    let dynamic = program.create("CaptureAttachmentsDemo")?;
    let app = CaptureAttachmentsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreter = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let work = |gpu: &Renderer| {
        let stats = gpu.scene_draw_stats();
        [
            stats.scene_capture_passes,
            stats.scene_capture_bytes_allocated,
            stats.scene_capture_geometry_draws,
            stats.custom_effect_surface_draws,
        ]
    };
    let pixels = |png: &[u8]| -> TestResult<[[u8; 3]; 6]> {
        let raster = xrack_assets::decode_png(png)?;
        Ok([
            (163, 140),
            (357, 140),
            (130, 36),
            (370, 36),
            (7, 36),
            (451, 47),
        ]
        .map(|(x, y)| {
            let start = 4 * (y * 520 + x);
            std::array::from_fn(|channel| raster.rgba[start + channel])
        }))
    };
    let mut draw = || -> TestResult<([[u8; 3]; 6], [u64; 4])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreter.render(&mut preview_gpu)?);
        assert_eq!(work(&gpu), work(&preview_gpu));
        Ok((pixels(png)?, work(&gpu)))
    };
    let (base, first) = draw()?;
    assert_eq!(
        base[0],
        [188, 188, 188],
        "orthographic near/far depth in material"
    );
    assert_eq!(
        base[1],
        [188, 231, 243],
        "programmable surface normal in material"
    );
    assert_eq!(base[2], base[0], "effect and material read the same depth");
    assert_eq!(
        base[3],
        [124, 203, 231],
        "roughness and normal channels in effect"
    );
    assert_eq!(base[4], [255; 3], "background depth clears to far");
    assert_eq!(first, [1, 260 * 140 * 12, 2, 0]);
    assert_eq!(draw()?.1, first);

    let mut app = weak.upgrade().ok_or("missing capture attachments demo")?;
    app.set_source_z(4.0);
    dynamic.set("source-z", Value::Float(4.0))?;
    let (moved, changed) = draw()?;
    assert_eq!(moved[0], [137; 3]);
    assert_eq!(moved[1], base[1]);
    assert_eq!(moved[2], moved[0]);
    assert_eq!(changed, [2, first[1], first[2] * 2, 0]);

    app.set_color_inset(true);
    dynamic.set("color-inset", Value::Bool(true))?;
    let (color, added) = draw()?;
    assert_eq!(&color[..5], &moved[..5]);
    assert_ne!(color[5], moved[5]);
    assert_eq!(added, [3, first[1] + 260 * 140 * 8, changed[2], 0]);

    // A depth-only configuration keeps the float depth target and skips the
    // normal MRT even when the same descriptor previously requested both.
    let mut depth_app = CaptureAttachmentsDemo::new();
    depth_app.set_diagnostic_strip(false);
    depth_app.set_normals(false);
    let depth_dynamic = program.create("CaptureAttachmentsDemo")?;
    depth_dynamic.set("diagnostic-strip", Value::Bool(false))?;
    depth_dynamic.set("normals", Value::Bool(false))?;
    let mut depth_aot = PreviewScene::new(Box::new(depth_app));
    let mut depth_interpreted = PreviewScene::new(Box::new(depth_dynamic));
    let (mut depth_gpu, mut depth_preview_gpu) = (renderer()?, renderer()?);
    let png = depth_aot.render(&mut depth_gpu)?;
    assert!(png == depth_interpreted.render(&mut depth_preview_gpu)?);
    assert_eq!(pixels(png)?[0], base[0]);
    assert_eq!(work(&depth_gpu), [1, 260 * 140 * 4, 2, 0]);
    assert_eq!(work(&depth_preview_gpu), work(&depth_gpu));
    Ok(())
}

#[test]
fn capture_world_positions_use_each_attachment_camera_and_reuse_projection_uploads() -> TestResult {
    use capture_positions_ui::CapturePositionsDemo;

    const NAME: &str = "examples/puzzle3d/capture_positions.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for asset in [
        "capture_position_material.wgsl",
        "capture_position_strip.wgsl",
    ] {
        assert!(inputs.iter().any(|path| path.ends_with(asset)));
    }
    let dynamic = program.create("CapturePositionsDemo")?;
    let app = CapturePositionsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 4], [u64; 4])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let raster = xrack_assets::decode_png(png)?;
        let pixels = [(172, 140), (368, 140), (135, 28), (405, 28)].map(|(x, y)| {
            let start = 4 * (y * 540 + x);
            std::array::from_fn(|channel| raster.rgba[start + channel])
        });
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.scene_capture_projection_uploads,
                stats.mesh_vertex_bytes_uploaded,
            ]
        };
        let stats = work(&gpu);
        assert_eq!(stats, work(&preview_gpu));
        Ok((pixels, stats))
    };
    let near = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 4),
            "{actual:?} vs {expected:?}"
        );
    };
    let (base, first) = draw()?;
    near(base[0], [150, 188, 153]);
    near(base[1], [218, 188, 184]);
    near(base[2], base[0]);
    near(base[3], base[1]);
    assert_eq!(&first[..2], &[2, 270 * 140 * 16]);
    assert_eq!(draw()?.1, first);

    let mut app = weak.upgrade().ok_or("missing capture positions demo")?;
    app.set_west_shift(0.6);
    dynamic.set("west-shift", Value::Float(0.6))?;
    let (camera, update) = draw()?;
    assert!(camera[0][0] > base[0][0] + 12);
    assert_eq!([camera[1], camera[3]], [base[1], base[3]]);
    near(camera[2], camera[0]);
    assert_eq!(
        update[1], first[1],
        "camera changes update an already allocated depth texture"
    );
    assert!(
        update[2] > first[2],
        "moving the camera uploads the new projection"
    );
    assert_eq!(update[3], first[3]);

    app.set_west_z(3.0);
    dynamic.set("west-z", Value::Float(3.0))?;
    let (geometry, newer) = draw()?;
    assert!(geometry[0][2] > camera[0][2] + 25);
    near(geometry[2], geometry[0]);
    assert_eq!(
        &newer[1..],
        &update[1..],
        "geometry updates reuse the camera uniforms"
    );

    app.set_asymmetric(true);
    dynamic.set("asymmetric", Value::Bool(true))?;
    let (perspective, _) = draw()?;
    assert_eq!([perspective[0], perspective[2]], [geometry[0], geometry[2]]);
    assert!(
        perspective[1][0] > geometry[1][0] + 7,
        "off-center lens: {perspective:?}"
    );
    assert!(perspective[1][2].abs_diff(geometry[1][2]) <= 2);
    near(perspective[3], perspective[1]);
    app.set_device_reversed(true);
    dynamic.set("device-reversed", Value::Bool(true))?;
    let (reversed, _) = draw()?;
    for (actual, expected) in reversed.into_iter().zip(perspective) {
        near(actual, expected);
    }
    Ok(())
}

#[test]
fn named_material_channels_relight_without_recapturing_geometry() -> TestResult {
    use capture_material_channels_ui::CaptureMaterialChannelsDemo;

    const NAME: &str = "examples/puzzle3d/capture_material_channels.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    for asset in [
        "capture_surface_swatch.wgsl",
        "capture_surface_inspector.wgsl",
        "capture_surface_light.wgsl",
        "channel-swatch.png",
    ] {
        assert!(inputs.iter().any(|path| path.ends_with(asset)));
    }
    let dynamic = program.create("CaptureMaterialChannelsDemo")?;
    let app = CaptureMaterialChannelsDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let pixels = |png: &[u8]| -> TestResult<[[u8; 3]; 6]> {
        let bitmap = xrack_assets::decode_png(png)?;
        Ok([
            (161, 122),
            (379, 122),
            (108, 270),
            (220, 270),
            (330, 270),
            (442, 270),
        ]
        .map(|(x, y)| {
            let start = 4 * (y * 540 + x);
            std::array::from_fn(|channel| bitmap.rgba[start + channel])
        }))
    };
    let work = |renderer: &Renderer| {
        let stats = renderer.scene_draw_stats();
        [
            stats.scene_capture_passes,
            stats.scene_capture_bytes_allocated,
            stats.scene_capture_geometry_draws,
            stats.scene_capture_projection_uploads,
            stats.mesh_vertex_bytes_uploaded,
        ]
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 6], [u64; 5])> {
        let png = aot.render(&mut gpu)?;
        assert!(png == interpreted.render(&mut preview_gpu)?);
        let stats = work(&gpu);
        assert_eq!(stats, work(&preview_gpu));
        Ok((pixels(png)?, stats))
    };
    let near = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{actual:?} vs {expected:?}"
        );
    };
    let (base, initial) = draw()?;
    near(base[2], [89, 160, 211]); // Linear RGB from scene_material.
    near(base[3], [137; 3]); // 0.25 metalness, stored separately from RGB.
    near(base[4], [39, 80, 118]); // HDR emission from scene_material.
    near(base[5], [99; 3]); // Raw alpha-channel occlusion from the PNG.
    assert_eq!(&initial[..4], &[1, 270 * 150 * 28, 2, 1]);

    let mut app = weak
        .upgrade()
        .ok_or("missing capture material channels demo")?;
    app.set_light_x(2.0);
    dynamic.set("light-x", Value::Float(2.0))?;
    let (lit, updated_light) = draw()?;
    assert!(base[0][2] > lit[0][2] + 18, "{base:?} -> {lit:?}");
    assert!(lit[1][0] > base[1][0] + 18, "{base:?} -> {lit:?}");
    assert_eq!(&lit[2..], &base[2..]);
    assert_eq!(
        updated_light, initial,
        "the light moves only in the post-effect"
    );

    app.set_metalness(0.8);
    dynamic.set("metalness", Value::Float(0.8))?;
    let (finish, changed) = draw()?;
    near(finish[3], [231; 3]);
    assert_eq!(
        [finish[2], finish[4], finish[5]],
        [base[2], base[4], base[5]]
    );
    assert_eq!(changed, [2, initial[1], 4, initial[3], initial[4]]);

    let mut emission_app = CaptureMaterialChannelsDemo::new();
    emission_app.set_emission_only(true);
    let emission_dynamic = program.create("CaptureMaterialChannelsDemo")?;
    emission_dynamic.set("emission-only", Value::Bool(true))?;
    let mut emission_aot = PreviewScene::new(Box::new(emission_app));
    let mut emission_interpreted = PreviewScene::new(Box::new(emission_dynamic));
    let (mut emission_gpu, mut emission_preview_gpu) = (renderer()?, renderer()?);
    let png = emission_aot.render(&mut emission_gpu)?;
    assert!(png == emission_interpreted.render(&mut emission_preview_gpu)?);
    near(pixels(png)?[0], base[4]);
    assert_eq!(&work(&emission_gpu)[..4], &[1, 270 * 150 * 8, 2, 0]);
    assert_eq!(work(&emission_gpu), work(&emission_preview_gpu));
    Ok(())
}

#[test]
fn named_capture_depth_occludes_forward_glass_in_both_backends() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, resources) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    assert!(
        resources
            .iter()
            .any(|path| path.ends_with("capture_composite_light.wgsl"))
    );
    let dynamic = program.create("CaptureCompositeDemo")?;
    let app = CaptureCompositeDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 5])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut preview_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [220, 72].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let work = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_composite_draws,
                stats.scene_capture_bytes_allocated,
                stats.transparency_composites,
                stats.screen_reflection_resolves,
            ]
        };
        let stats = work(&gpu);
        assert_eq!(stats, work(&preview_gpu));
        Ok((pixels, stats))
    };
    let near = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{actual:?} vs {expected:?}"
        );
    };
    let (occluded, first) = draw()?;
    assert!(occluded[0][2] > occluded[0][0] + 85, "{occluded:?}");
    assert!(occluded[1][0] > occluded[1][2] + 55, "{occluded:?}");
    assert_eq!(first, [2, 1, 480 * 288 * 36, 0, 0]);
    assert_eq!(draw()?.1, first, "clean view reuses the completed frame");

    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;
    app.set_share_depth(false);
    dynamic.set("share-depth", Value::Bool(false))?;
    let (color_only, second) = draw()?;
    assert!(color_only[0][0] > occluded[0][0] + 85, "{color_only:?}");
    near(color_only[1], occluded[1]);
    assert_eq!(
        second,
        [2, 2, first[2], 0, 0],
        "switching depth reuses both captures"
    );

    app.set_share_depth(true);
    dynamic.set("share-depth", Value::Bool(true))?;
    app.set_different_camera(true);
    dynamic.set("different-camera", Value::Bool(true))?;
    let (mismatched, work) = draw()?;
    assert_eq!(
        mismatched, color_only,
        "foreign projection cannot seed viewport depth"
    );
    assert_eq!(
        work,
        [2, 3, first[2], 0, 0],
        "incompatible depth never allocates a target"
    );

    app.set_different_camera(false);
    dynamic.set("different-camera", Value::Bool(false))?;
    app.set_glass_depth(2.0);
    dynamic.set("glass-depth", Value::Float(2.0))?;
    let (foreground, _) = draw()?;
    near(foreground[0], color_only[0]);
    near(foreground[1], color_only[1]);

    app.set_weighted(true);
    dynamic.set("weighted", Value::Bool(true))?;
    app.set_antialias(true);
    dynamic.set("antialias", Value::Bool(true))?;
    app.set_reflections(true);
    dynamic.set("reflections", Value::Bool(true))?;
    let (weighted_front, weighted) = draw()?;
    near(weighted_front[0], foreground[0]);
    near(weighted_front[1], foreground[1]);
    assert!(weighted[3] > 0 && weighted[4] > 0, "{weighted:?}");
    app.set_glass_depth(0.0);
    dynamic.set("glass-depth", Value::Float(0.0))?;
    let (weighted_back, _) = draw()?;
    near(weighted_back[0], occluded[0]);
    near(weighted_back[1], occluded[1]);

    app.set_use_color(false);
    dynamic.set("use-color", Value::Bool(false))?;
    let (depth_only, _) = draw()?;
    near(depth_only[0], [25, 45, 68]);
    assert!(depth_only[1][0] > depth_only[1][2] + 55, "{depth_only:?}");
    app.set_share_depth(false);
    dynamic.set("share-depth", Value::Bool(false))?;
    let (normal, _) = draw()?;
    assert!(normal[0][0] > depth_only[0][0] + 85, "{normal:?}");

    // Editor previews use fractional pixel scales. The capture must keep the
    // viewport's logical camera even when the GPU texture aspect rounds.
    let mut aot_scaled = PreviewScene::new(Box::new(CaptureCompositeDemo::new()));
    let mut interp_scaled = PreviewScene::new(Box::new(program.create("CaptureCompositeDemo")?));
    let scaled = aot_scaled.render_scaled(&mut gpu, 1.37)?;
    assert!(scaled == interp_scaled.render_scaled(&mut preview_gpu, 1.37)?);
    let bitmap = xrack_assets::decode_png(scaled)?;
    let [width, height] = bitmap.size.map(|side| side as usize);
    let index = 4 * ((144 * height / 350) * width + 220 * width / 480);
    let scaled_center = std::array::from_fn(|channel| bitmap.rgba[index + channel]);
    near(scaled_center, occluded[0]);
    Ok(())
}

#[test]
fn color_captures_load_upstream_attachments_and_share_resolved_aliases() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    dynamic.set("offscreen-composition", Value::Bool(true))?;
    dynamic.set("compare-capture-alias", Value::Bool(true))?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_compare_capture_alias(true);
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut preview_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 4])> {
        let rendered = aot.render(&mut gpu)?;
        assert!(rendered == interpreted.render(&mut preview_gpu)?);
        let image = xrack_assets::decode_png(rendered)?;
        let pixels = [220, 72].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.scene_capture_composite_draws,
                stats.scene_capture_feedback_edges,
            ]
        };
        let work = counts(&gpu);
        assert_eq!(work, counts(&preview_gpu));
        Ok((pixels, work))
    };
    let (occluded, first) = draw()?;
    assert!(occluded[0][2] > occluded[0][0] + 85, "{occluded:?}");
    assert!(occluded[1][0] > occluded[1][2] + 55, "{occluded:?}");
    assert_eq!(
        first,
        [3, 480 * 288 * 44, 2, 0],
        "two aliases share one forward pass"
    );
    assert_eq!(draw()?.1, first);

    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;
    app.set_share_depth(false);
    dynamic.set("share-depth", Value::Bool(false))?;
    let (no_depth, unchanged_allocation) = draw()?;
    assert!(no_depth[0][0] > occluded[0][0] + 85, "{no_depth:?}");
    assert_eq!(no_depth[1], occluded[1]);
    assert_eq!(unchanged_allocation, [4, first[1], first[2] + 2, 0]);

    app.set_share_depth(true);
    dynamic.set("share-depth", Value::Bool(true))?;
    app.set_different_camera(true);
    dynamic.set("different-camera", Value::Bool(true))?;
    assert_eq!(draw()?, (no_depth, [4, first[1], first[2] + 3, 0]));

    app.set_different_camera(false);
    dynamic.set("different-camera", Value::Bool(false))?;
    app.set_compare_capture_alias(false);
    dynamic.set("compare-capture-alias", Value::Bool(false))?;
    app.set_lighting_source("relay".to_string());
    dynamic.set("lighting-source", Value::String("relay".to_string()))?;
    let (cycle, work) = draw()?;
    assert!(
        cycle[0]
            .into_iter()
            .zip([25, 45, 68])
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "{cycle:?}"
    );
    assert!(cycle[1][0] > cycle[1][2] + 55, "{cycle:?}");
    assert_eq!(work[3], 2, "both feedback edges are removed");
    assert_eq!(
        work[1], first[1],
        "unused relay never allocates a color target"
    );

    app.set_lighting_source("lighting".to_string());
    dynamic.set("lighting-source", Value::String("lighting".to_string()))?;
    assert_eq!(draw()?.0, occluded);
    Ok(())
}

#[test]
fn named_capture_renderables_filter_sections_and_skip_unused_material_inputs() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    dynamic.set("offscreen-composition", Value::Bool(true))?;
    dynamic.set("mixed-mesh", Value::Bool(true))?;
    dynamic.set("capture-renderables", Value::String("transparent".into()))?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_mixed_mesh(true);
    app.set_capture_renderables("transparent".into());
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let new_renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (new_renderer()?, new_renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 4])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [110, 240].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.color_instances,
                stats.material_depth_prepass_draws,
            ]
        };
        let work = counts(&gpu);
        assert_eq!(work, counts(&other_gpu));
        Ok((pixels, work))
    };
    let (transparent, first) = draw()?;
    assert!(
        transparent[0][2] > transparent[0][0] + 70,
        "{transparent:?}"
    );
    assert!(transparent[1][0] > 120, "{transparent:?}");
    assert_eq!(
        first[0..2],
        [3, 480 * 288 * 44],
        "the excluded masked part does not request its color capture"
    );
    assert_eq!(draw()?.1, first);

    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;
    app.set_capture_renderables("opaque".into());
    dynamic.set("capture-renderables", Value::String("opaque".into()))?;
    let (opaque, second) = draw()?;
    assert!(opaque[0][1] > opaque[0][2] + 70, "{opaque:?}");
    assert!(opaque[1][2] > opaque[1][0] + 70, "{opaque:?}");
    assert_eq!(
        second[0..2],
        [5, 480 * 288 * 52],
        "one upstream target and the selected forward pass"
    );
    assert_eq!(
        second[2] - first[2],
        2,
        "one accent producer and one masked part"
    );
    assert_eq!(
        second[3] - first[3],
        1,
        "only the selected masked part reaches the prepass"
    );

    app.set_capture_renderables("none".into());
    dynamic.set("capture-renderables", Value::String("none".into()))?;
    let (inputs, third) = draw()?;
    assert_eq!(inputs, [transparent[0], opaque[1]]);
    assert_eq!(
        third,
        [6, second[1], second[2], second[3]],
        "composition-only mode submits no geometry or depth prepass"
    );

    app.set_capture_renderables("all".into());
    dynamic.set("capture-renderables", Value::String("all".into()))?;
    let (all, fourth) = draw()?;
    assert_eq!(all, [opaque[0], transparent[1]]);
    assert_eq!(
        fourth[0..2],
        [7, second[1]],
        "all stages reuse the named targets"
    );
    assert_eq!(
        fourth[2] - third[2],
        3,
        "three sections of the original glTF"
    );
    Ok(())
}

#[test]
fn capture_material_override_filters_original_sections_and_reuses_mesh_streams() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    for field in ["offscreen-composition", "mixed-mesh", "override-enabled"] {
        dynamic.set(field, Value::Bool(true))?;
    }
    dynamic.set("capture-renderables", Value::String("opaque".into()))?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_mixed_mesh(true);
    app.set_override_enabled(true);
    app.set_capture_renderables("opaque".into());
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let new_renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (new_renderer()?, new_renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 4])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [110, 240].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.mesh_index_bytes_uploaded,
            ]
        };
        let work = counts(&gpu);
        assert_eq!(work, counts(&other_gpu));
        Ok((pixels, work))
    };
    let near = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "{actual:?} vs {expected:?}"
        );
    };
    let (opaque, first) = draw()?;
    near(opaque[0], [226, 68, 147]);
    assert!(opaque[1][2] > opaque[1][0] + 70, "{opaque:?}");
    assert_eq!(
        first[0..2],
        [3, 480 * 288 * 44],
        "the original material's capture is unused"
    );
    assert_eq!(draw()?.1, first);

    let mut app = weak.upgrade().ok_or("missing capture demo")?;
    app.set_override_color(xrack_runtime::Color(0x33a9f4ff));
    dynamic.set(
        "override-color",
        Value::Color(xrack_runtime::Color(0x33a9f4ff)),
    )?;
    let (new_color, recolored) = draw()?;
    near(new_color[0], [51, 169, 244]);
    assert_eq!(new_color[1], opaque[1]);
    assert_eq!(
        recolored,
        [4, first[1], first[2], first[3]],
        "changing the material keeps shared vertex streams"
    );

    app.set_capture_renderables("transparent".into());
    dynamic.set("capture-renderables", Value::String("transparent".into()))?;
    let (blended_sections, filtered) = draw()?;
    assert!(
        blended_sections[0][2] > blended_sections[0][0] + 70,
        "{blended_sections:?}"
    );
    near(blended_sections[1], [51, 169, 244]);
    assert_eq!(filtered, [5, first[1], first[2], first[3]]);

    app.set_override_shader(true);
    dynamic.set("override-shader", Value::Bool(true))?;
    let (shader, after) = draw()?;
    near(shader[1], [66, 228, 109]);
    assert_eq!(shader[0], blended_sections[0]);
    assert_eq!(after[0..2], [7, first[1] + 480 * 288 * 8]);
    Ok(())
}

#[test]
fn capture_pipeline_state_controls_depth_culling_and_direct_blending() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    dynamic.set("offscreen-composition", Value::Bool(true))?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw_number = 0;
    let mut draw = || -> TestResult<([[u8; 3]; 4], [u64; 5])> {
        let image = aot.render(&mut gpu)?;
        draw_number += 1;
        assert!(
            image == interpreted.render(&mut other_gpu)?,
            "different output on draw {draw_number}"
        );
        let image = xrack_assets::decode_png(image)?;
        let pixels = [(220, 144), (72, 144), (326, 54), (397, 54)].map(|(x, y)| {
            let start = 4 * (y * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.transparency_composites,
                stats.particle_model_draws,
            ]
        };
        let stats = counts(&gpu);
        assert_eq!(stats, counts(&other_gpu));
        Ok((pixels, stats))
    };
    let (occluded, first) = draw()?;
    assert!(occluded[0][2] > occluded[0][0] + 80);
    assert!(occluded[1][0] > occluded[1][2] + 55);

    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;
    app.set_pipeline_enabled(true);
    dynamic.set("pipeline-enabled", Value::Bool(true))?;
    let (bypassed, second) = draw()?;
    assert!(bypassed[0][0] > occluded[0][0] + 100);
    assert_eq!(bypassed[1], occluded[1]);
    assert_eq!(
        second,
        [first[0] + 1, first[1], first[2], first[3], first[4]]
    );

    app.set_capture_depth_test(true);
    dynamic.set("capture-depth-test", Value::Bool(true))?;
    app.set_capture_depth_compare("greater".into());
    dynamic.set("capture-depth-compare", Value::String("greater".into()))?;
    let (farther, _) = draw()?;
    assert_eq!(farther[0], bypassed[0], "test against the imported depth");
    assert!(farther[1][0] < 40, "{farther:?}");

    app.set_capture_cull_mode("front".into());
    dynamic.set("capture-cull-mode", Value::String("front".into()))?;
    assert_eq!(draw()?.0[0], occluded[0], "front-facing glass is culled");

    app.set_capture_depth_test(false);
    dynamic.set("capture-depth-test", Value::Bool(false))?;
    app.set_capture_cull_mode("material".into());
    dynamic.set("capture-cull-mode", Value::String("material".into()))?;
    app.set_weighted(true);
    dynamic.set("weighted", Value::Bool(true))?;
    app.set_capture_blend_mode("replace".into());
    dynamic.set("capture-blend-mode", Value::String("replace".into()))?;
    let (replace, ordered) = draw()?;
    assert!(replace[0][2] + 50 < bypassed[0][2], "{replace:?}");
    assert_eq!(
        ordered[3], first[3],
        "a fixed blend state uses direct color output"
    );

    app.set_capture_blend_mode("additive".into());
    dynamic.set("capture-blend-mode", Value::String("additive".into()))?;
    app.set_particle_fixture(true);
    dynamic.set("particle-fixture", Value::Bool(true))?;
    let (additive, particles) = draw()?;
    assert!(additive[0][2] > bypassed[0][2] + 30, "{additive:?}");
    assert!(additive[2][0] > occluded[2][0] + 30, "{additive:?}");
    assert!(additive[3][0] > occluded[3][0] + 30, "{additive:?}");
    assert_eq!(particles[3], first[3]);
    assert!(
        particles[4] > ordered[4],
        "translucent model particles reach the direct pass"
    );

    app.set_particle_fixture(false);
    dynamic.set("particle-fixture", Value::Bool(false))?;
    app.set_depth_fixture(true);
    dynamic.set("depth-fixture", Value::Bool(true))?;
    app.set_share_depth(false);
    dynamic.set("share-depth", Value::Bool(false))?;
    app.set_use_color(false);
    dynamic.set("use-color", Value::Bool(false))?;
    app.set_capture_blend_mode("material".into());
    dynamic.set("capture-blend-mode", Value::String("material".into()))?;
    app.set_capture_depth_test(true);
    dynamic.set("capture-depth-test", Value::Bool(true))?;
    app.set_capture_depth_compare("less".into());
    dynamic.set("capture-depth-compare", Value::String("less".into()))?;
    let green = draw()?.0[0];
    assert!(green[1] > green[0] + 95, "{green:?}");
    app.set_capture_depth_write(true);
    dynamic.set("capture-depth-write", Value::Bool(true))?;
    let red = draw()?.0[0];
    assert!(red[0] > red[1] + 130, "{red:?}");
    Ok(())
}

#[test]
fn capture_viewport_and_scissor_align_color_depth_and_world_positions() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    dynamic.set("offscreen-composition", Value::Bool(true))?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 4], [u64; 5])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [220, 72, 270, 192].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.color_instances,
                stats.transparency_composites,
            ]
        };
        let stats = counts(&gpu);
        assert_eq!(stats, counts(&other_gpu));
        Ok((pixels, stats))
    };
    let (occluded, first) = draw()?;
    assert!(occluded[0][2] > occluded[0][0] + 80);
    assert!(occluded[1][0] > occluded[1][2] + 55);
    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;

    app.set_pipeline_enabled(true);
    dynamic.set("pipeline-enabled", Value::Bool(true))?;
    app.set_capture_viewport(true);
    dynamic.set("capture-viewport", Value::Bool(true))?;
    let (inset, moved) = draw()?;
    assert!(inset[0][0] > occluded[0][0] + 100);
    assert!(inset[1][0] < 40, "{inset:?}");
    assert_eq!(moved[..3], [first[0] + 1, first[1], first[2]]);

    app.set_capture_scissor(true);
    dynamic.set("capture-scissor", Value::Bool(true))?;
    let (clipped, _) = draw()?;
    assert_eq!(clipped, [occluded[0], inset[1], inset[2], occluded[3]]);

    app.set_weighted(true);
    dynamic.set("weighted", Value::Bool(true))?;
    let (weighted, work) = draw()?;
    assert_eq!(
        [weighted[0], weighted[1], weighted[3]],
        [clipped[0], clipped[1], clipped[3]]
    );
    assert!(weighted[2][0] > occluded[2][0] + 80);
    assert!(work[4] > moved[4]);

    app.set_capture_scissor_width(0.2501);
    dynamic.set("capture-scissor-width", Value::Float(0.2501))?;
    assert_eq!(
        draw()?,
        (weighted, work),
        "the same scissor pixels reuse the capture"
    );

    app.set_capture_scissor_width(0.0);
    dynamic.set("capture-scissor-width", Value::Float(0.0))?;
    let (empty, outside) = draw()?;
    assert_eq!(empty, [occluded[0], inset[1], occluded[2], occluded[3]]);
    assert_eq!(outside, [work[0] + 1, work[1], work[2], work[3], work[4]]);

    app.set_capture_scissor(false);
    dynamic.set("capture-scissor", Value::Bool(false))?;
    app.set_capture_depth_test(true);
    dynamic.set("capture-depth-test", Value::Bool(true))?;
    let (foreign, _) = draw()?;
    assert!(
        foreign[0][0] > occluded[0][0] + 90,
        "depth from the original viewport cannot occlude an inset"
    );

    app.set_map_surface(true);
    dynamic.set("map-surface", Value::Bool(true))?;
    let (matched, _) = draw()?;
    assert!(
        matched[0][2] > matched[0][0] + 90,
        "matching raster projections share depth"
    );

    app.set_capture_renderables("none".into());
    dynamic.set("capture-renderables", Value::String("none".into()))?;
    app.set_probe_position(true);
    dynamic.set("probe-position", Value::Bool(true))?;
    let (position, _) = draw()?;
    let check_position = |actual: [u8; 3]| {
        // Pixel (192,144) maps to approximately (-1.32, -0.014, 1)
        // on the inset orthographic plane before channel encoding.
        assert!(
            actual
                .into_iter()
                .zip([157, 187, 99])
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{actual:?}"
        );
    };
    check_position(position[3]);
    let image = aot.render_scaled(&mut gpu, 1.37)?;
    assert!(image == interpreted.render_scaled(&mut other_gpu, 1.37)?);
    let image = xrack_assets::decode_png(image)?;
    let [width, height] = image.size.map(|side| side as usize);
    let start = 4 * ((144 * height / 350) * width + 192 * width / 480);
    check_position(std::array::from_fn(|channel| image.rgba[start + channel]));
    Ok(())
}

#[test]
fn named_depth_clear_seeds_comparison_and_preserves_invalid_positions() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_pipeline_enabled(true);
    app.set_capture_depth_test(true);
    app.set_capture_depth_compare("greater".into());
    app.set_share_depth(false);
    for field in [
        "offscreen-composition",
        "pipeline-enabled",
        "capture-depth-test",
    ] {
        dynamic.set(field, Value::Bool(true))?;
    }
    dynamic.set("capture-depth-compare", Value::String("greater".into()))?;
    dynamic.set("share-depth", Value::Bool(false))?;
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 3])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [220, 72].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
            ]
        };
        let stats = counts(&gpu);
        assert_eq!(stats, counts(&other_gpu));
        Ok((pixels, stats))
    };
    let near = |actual: [u8; 3], expected: [u8; 3]| {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 3),
            "{actual:?} vs {expected:?}"
        );
    };
    let (far, first) = draw()?;
    assert!(far[0][2] > far[0][0] + 80 && far[1][0] < 40, "{far:?}");
    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;

    app.set_capture_depth_clear(0.0);
    dynamic.set("capture-depth-clear", Value::Float(0.0))?;
    let (visible, near_stats) = draw()?;
    assert!(visible[0][0] > far[0][0] + 100 && visible[1][0] > far[1][0] + 100);
    assert_eq!(near_stats, [first[0] + 1, first[1], first[2]]);

    app.set_capture_depth_clear(1.5);
    dynamic.set("capture-depth-clear", Value::Float(1.5))?;
    assert_eq!(draw()?.0, far);
    app.set_share_depth(true);
    dynamic.set("share-depth", Value::Bool(true))?;
    app.set_seed_depth(true);
    dynamic.set("seed-depth", Value::Bool(true))?;
    let (imported, seed_stats) = draw()?;
    assert_eq!(
        imported, visible,
        "a capture with no renderables can seed depth"
    );
    assert_eq!(
        seed_stats[..2],
        [near_stats[0] + 3, first[1] + 480 * 288 * 4]
    );
    app.set_seed_depth_clear(0.75);
    dynamic.set("seed-depth-clear", Value::Float(0.75))?;
    let (rejected, later) = draw()?;
    assert_eq!(rejected, far);
    assert_eq!(later, [seed_stats[0] + 2, seed_stats[1], seed_stats[2]]);

    app.set_seed_depth_clear(0.0);
    dynamic.set("seed-depth-clear", Value::Float(0.0))?;
    assert_eq!(draw()?.0, visible, "zero is a valid imported clear");

    app.set_use_color(false);
    dynamic.set("use-color", Value::Bool(false))?;
    let (_, seed_only) = draw()?;
    app.set_glass_depth(0.2);
    dynamic.set("glass-depth", Value::Float(0.2))?;
    let (_, moved) = draw()?;
    assert_eq!(
        moved,
        [seed_only[0] + 1, seed_only[1], seed_only[2]],
        "moving the glass reuses the uniform producer"
    );
    app.set_use_color(true);
    dynamic.set("use-color", Value::Bool(true))?;

    app.set_capture_renderables("none".into());
    dynamic.set("capture-renderables", Value::String("none".into()))?;
    app.set_surface_depth_clear(0.24);
    dynamic.set("surface-depth-clear", Value::Float(0.24))?;
    app.set_inspect_clear(true);
    dynamic.set("inspect-clear", Value::Bool(true))?;
    let (decoded, _) = draw()?;
    near(decoded[0], [180, 231, 203]);
    near(decoded[1], [134, 124, 89]);
    app.set_effect_inspect_clear(true);
    dynamic.set("effect-inspect-clear", Value::Bool(true))?;
    let (effect, _) = draw()?;
    near(effect[0], decoded[0]);
    near(effect[1], decoded[1]);
    app.set_surface_depth_clear(5.0 / 11.0);
    dynamic.set("surface-depth-clear", Value::Float(5.0 / 11.0))?;
    let (equal, _) = draw()?;
    near(equal[0], decoded[0]);
    near(equal[1], [180, 124, 89]);

    app.set_surface_depth_clear(0.0);
    dynamic.set("surface-depth-clear", Value::Float(0.0))?;
    let (effect_zero, _) = draw()?;
    near(effect_zero[0], decoded[0]);
    near(effect_zero[1], [0, 124, 89]);
    app.set_effect_inspect_clear(false);
    dynamic.set("effect-inspect-clear", Value::Bool(false))?;
    let (material_zero, _) = draw()?;
    near(material_zero[1], effect_zero[1]);
    Ok(())
}

#[test]
fn named_stencil_capture_masks_another_camera_and_reuses_the_output() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    dynamic.set("offscreen-composition", Value::Bool(true))?;
    dynamic.set("stencil-demo", Value::Bool(true))?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_stencil_demo(true);
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 5])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [220, 320].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.scene_capture_stencil_copies,
                stats.mesh_vertex_bytes_uploaded,
                stats.transparency_composites,
            ]
        };
        let stats = counts(&gpu);
        assert_eq!(stats, counts(&other_gpu));
        Ok((pixels, stats))
    };
    let (masked, first) = draw()?;
    assert!(
        masked[0][0] > 150 && masked[1][2] > masked[1][0] + 90,
        "{masked:?}"
    );
    assert_eq!(first[..3], [4, 480 * 288 * 52, 1]);
    assert_eq!(
        draw()?.1,
        first,
        "completed captures need no stencil transfer"
    );
    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;

    app.set_stencil_read_ref(2);
    dynamic.set("stencil-read-ref", Value::Int(2))?;
    let (outside, changed) = draw()?;
    assert!(
        outside[0][2] > outside[0][0] + 90 && outside[1][0] > 150,
        "{outside:?}"
    );
    let background = [outside[0], masked[1]];
    assert_eq!(
        changed,
        [first[0] + 1, first[1], first[2] + 1, first[3], first[4]]
    );

    app.set_stencil_read_ref(3);
    dynamic.set("stencil-read-ref", Value::Int(3))?;
    app.set_stencil_compare("not-equal".into());
    dynamic.set("stencil-compare", Value::String("not-equal".into()))?;
    let (inverted, before_fail) = draw()?;
    assert_eq!(inverted[0], background[0]);
    assert!(inverted[1][0] > 150, "{inverted:?}");

    app.set_stencil_depth_fail(true);
    dynamic.set("stencil-depth-fail", Value::Bool(true))?;
    let (depth_fail, after_fail) = draw()?;
    assert_eq!(
        depth_fail, inverted,
        "the mask can be written by the depth-fail operation"
    );
    assert_eq!(
        after_fail,
        [
            before_fail[0] + 2,
            before_fail[1],
            before_fail[2] + 1,
            before_fail[3],
            before_fail[4]
        ]
    );

    app.set_weighted(true);
    dynamic.set("weighted", Value::Bool(true))?;
    let (weighted, weight_stats) = draw()?;
    assert_eq!(
        weighted[0], inverted[0],
        "weighted transparency retains the stencil attachment"
    );
    assert!(weighted[1][0] > 150, "{weighted:?}");
    assert!(weight_stats[4] > after_fail[4]);

    app.set_stencil_compare("equal".into());
    dynamic.set("stencil-compare", Value::String("equal".into()))?;
    app.set_stencil_mask_scale(0.5);
    dynamic.set("stencil-mask-scale", Value::Float(0.5))?;
    let (mismatched, mismatch_stats) = draw()?;
    assert_eq!(mismatched, background);
    assert_eq!(
        mismatch_stats[..3],
        [weight_stats[0] + 1, weight_stats[1], weight_stats[2]]
    );
    Ok(())
}

#[test]
fn capture_wireframe_rasterizes_edges_through_stencil_and_reuses_meshes() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_stencil_demo(true);
    for field in ["offscreen-composition", "stencil-demo"] {
        dynamic.set(field, Value::Bool(true))?;
    }
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([usize; 3], [u64; 5])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let tint = |start: usize, end: usize| {
            (86..205)
                .map(|y| {
                    (start..end)
                        .filter(|x| image.rgba[4 * (480 * y + x)] > 150)
                        .count()
                })
                .sum()
        };
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.mesh_wireframe_uploads,
                stats.particle_draws,
            ]
        };
        let stats = counts(&gpu);
        assert_eq!(stats, counts(&other_gpu));
        Ok(([tint(112, 270), tint(278, 382), tint(112, 235)], stats))
    };
    let (fill, initial) = draw()?;
    assert!(fill[0] > 10_000 && fill[1] == 0, "{fill:?}");
    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;
    app.set_capture_polygon_mode("line".into());
    dynamic.set("capture-polygon-mode", Value::String("line".into()))?;
    let (line, stencil) = draw()?;
    assert!(line[0] > 100 && line[0] < fill[0] / 8, "{line:?}");
    assert_eq!(line[1], 0, "line fragments obey the imported stencil");
    assert_eq!(
        stencil,
        [
            initial[0] + 1,
            initial[1],
            initial[2],
            initial[3],
            initial[4]
        ]
    );
    assert_eq!(draw()?.1, stencil);

    app.set_stencil_demo(false);
    dynamic.set("stencil-demo", Value::Bool(false))?;
    app.set_pipeline_enabled(true);
    dynamic.set("pipeline-enabled", Value::Bool(true))?;
    app.set_particle_fixture(true);
    dynamic.set("particle-fixture", Value::Bool(true))?;
    app.set_capture_polygon_mode("fill".into());
    dynamic.set("capture-polygon-mode", Value::String("fill".into()))?;
    let (_, solid) = draw()?;
    app.set_capture_polygon_mode("line".into());
    dynamic.set("capture-polygon-mode", Value::String("line".into()))?;
    let (_, wire) = draw()?;
    assert_eq!(wire[1], solid[1]);
    assert!(
        wire[4] > solid[4],
        "sprite particles reach the wireframe pass"
    );
    if std::env::var_os("XRACK_FORCE_SHADER_WIREFRAME").is_some_and(|value| value == "1") {
        assert!(wire[3] > solid[3] && wire[2] > solid[2], "{wire:?}");
    } else if wire[3] == solid[3] {
        assert_eq!(
            wire[2], solid[2],
            "native polygon lines retain indexed vertices"
        );
    }
    app.set_capture_polygon_mode("fill".into());
    dynamic.set("capture-polygon-mode", Value::String("fill".into()))?;
    assert_eq!(draw()?.1[2..4], wire[2..4]);
    app.set_capture_polygon_mode("line".into());
    dynamic.set("capture-polygon-mode", Value::String("line".into()))?;
    assert_eq!(draw()?.1[2..4], wire[2..4]);
    app.set_capture_scissor(true);
    dynamic.set("capture-scissor", Value::Bool(true))?;
    let (clipped, reused) = draw()?;
    assert_eq!(
        clipped[2], 0,
        "the scissor excludes wireframe fragments on the left"
    );
    assert!(clipped[1] > 100, "{clipped:?}");
    assert_eq!(reused[2..4], wire[2..4]);
    Ok(())
}

#[test]
fn capture_target_blend_writes_selected_channels_and_reuses_fixed_state() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_pipeline_enabled(true);
    app.set_custom_blend(true);
    app.set_particle_fixture(true);
    app.set_weighted(true);
    app.set_capture_write_mask(0);
    let red = xrack_runtime::Color(0xff00_0066);
    app.set_capture_blend_constant(red);
    for field in [
        "offscreen-composition",
        "pipeline-enabled",
        "custom-blend",
        "particle-fixture",
        "weighted",
    ] {
        dynamic.set(field, Value::Bool(true))?;
    }
    dynamic.set("capture-write-mask", Value::Int(0))?;
    dynamic.set("capture-blend-constant", Value::Color(red))?;
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 3], [u64; 5])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [(220, 144), (326, 54), (397, 54)].map(|(x, y)| {
            let start = 4 * (y * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.transparency_composites,
                stats.custom_material_pipeline_builds,
            ]
        };
        let stats = counts(&gpu);
        assert_eq!(stats, counts(&other_gpu));
        Ok((pixels, stats))
    };
    let (unchanged, initial) = draw()?;
    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;
    app.set_capture_write_mask(1);
    dynamic.set("capture-write-mask", Value::Int(1))?;
    let (added, red_only) = draw()?;
    for (after, before) in added.into_iter().zip(unchanged) {
        assert!(after[0] > before[0] + 20, "{added:?} {unchanged:?}");
        assert_eq!(after[1..], before[1..], "the RGB mask retains G and B");
    }
    assert_eq!(
        red_only[1..4],
        initial[1..4],
        "fixed blending reuses the capture and draws directly"
    );
    app.set_capture_color_operation("reverse-subtract".into());
    dynamic.set(
        "capture-color-operation",
        Value::String("reverse-subtract".into()),
    )?;
    let (subtracted, _) = draw()?;
    assert!(subtracted[0][0] + 10 < unchanged[0][0], "{subtracted:?}");
    assert_eq!(subtracted[0][1..], unchanged[0][1..]);

    app.set_capture_color_operation("add".into());
    dynamic.set("capture-color-operation", Value::String("add".into()))?;
    app.set_inspect_blend_alpha(true);
    dynamic.set("inspect-blend-alpha", Value::Bool(true))?;
    let (alpha_preserved, _) = draw()?;
    app.set_capture_write_mask(15);
    dynamic.set("capture-write-mask", Value::Int(15))?;
    let (scaled_alpha, _) = draw()?;
    assert!(scaled_alpha[0][0] + 70 < alpha_preserved[0][0]);
    assert_eq!(scaled_alpha[0], [scaled_alpha[0][0]; 3]);
    app.set_capture_alpha_operation("max".into());
    dynamic.set("capture-alpha-operation", Value::String("max".into()))?;
    assert_eq!(draw()?.0[0], alpha_preserved[0], "max compares raw alpha");
    app.set_capture_blend_enabled(false);
    dynamic.set("capture-blend-enabled", Value::Bool(false))?;
    let (unblended_alpha, _) = draw()?;
    assert!(unblended_alpha[0][0] > scaled_alpha[0][0] + 30);
    assert!(unblended_alpha[0][0] + 20 < alpha_preserved[0][0]);

    app.set_capture_blend_enabled(true);
    app.set_inspect_blend_alpha(false);
    app.set_override_enabled(true);
    app.set_override_shader(true);
    for (field, value) in [
        ("capture-blend-enabled", true),
        ("inspect-blend-alpha", false),
        ("override-enabled", true),
        ("override-shader", true),
    ] {
        dynamic.set(field, Value::Bool(value))?;
    }
    let (before, compiled) = draw()?;
    assert!(compiled[4] > initial[4]);
    let green = xrack_runtime::Color(0x00ff_0066);
    app.set_capture_blend_constant(green);
    dynamic.set("capture-blend-constant", Value::Color(green))?;
    let (after, reused) = draw()?;
    assert_ne!(after, before);
    assert_eq!(reused[0], compiled[0] + 1);
    assert_eq!(
        reused[1..],
        compiled[1..],
        "changing the dynamic constant reuses geometry, textures and pipelines"
    );
    assert_eq!(draw()?.1, reused);
    Ok(())
}

#[test]
fn capture_color_formats_keep_attachment_sizes_and_separate_incompatible_aliases() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    const PIXELS: u64 = 480 * 288;
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_compare_capture_alias(true);
    app.set_weighted(true);
    for field in ["offscreen-composition", "compare-capture-alias", "weighted"] {
        dynamic.set(field, Value::Bool(true))?;
    }
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 6])> {
        let image = aot.render(&mut gpu)?;
        assert!(image == interpreted.render(&mut other_gpu)?);
        let image = xrack_assets::decode_png(image)?;
        let pixels = [220, 72].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.transparency_composites,
                stats.transmission_captures,
                stats.transmission_mip_passes,
            ]
        };
        let work = counts(&gpu);
        assert_eq!(work, counts(&other_gpu));
        Ok((pixels, work))
    };
    let (hdr, mut work) = draw()?;
    assert!(
        hdr[0][2] > hdr[0][0] + 60 && work[3] > 0,
        "{hdr:?} {work:?}"
    );
    let mut app = weak.upgrade().ok_or("missing capture composition demo")?;
    for (format, channels, bytes) in [
        ("rgba8", 3, 4),
        ("r8", 1, 1),
        ("r16f", 1, 2),
        ("rg8", 2, 2),
        ("rg16f", 2, 4),
        ("rgba16f", 3, 8),
    ] {
        app.set_capture_color_format(format.into());
        dynamic.set("capture-color-format", Value::String(format.into()))?;
        let (pixels, next) = draw()?;
        for (actual, reference) in pixels.into_iter().zip(hdr) {
            assert!(
                actual[..channels]
                    .iter()
                    .zip(&reference[..channels])
                    .all(|(&a, &b)| a.abs_diff(b) <= 6),
                "{format} {pixels:?} {hdr:?}"
            );
            assert!(actual[channels..].iter().all(|&channel| channel == 0));
        }
        assert_eq!(
            next,
            [
                work[0] + 1,
                work[1] + PIXELS * bytes,
                work[2],
                work[3] + 1,
                work[4],
                work[5],
            ],
            "{format} only recreates the shared color attachment"
        );
        assert_eq!(draw()?.1, next);
        work = next;
    }

    app.set_capture_alias_color_format("r8".into());
    dynamic.set("capture-alias-color-format", Value::String("r8".into()))?;
    let (distinct, split) = draw()?;
    assert_eq!(
        distinct[0],
        [255, 0, 255],
        "shader sees distinct alias data"
    );
    assert_eq!(split[0], work[0] + 1);
    assert_eq!(split[1], work[1] + PIXELS);
    app.set_compare_capture_alias(false);
    dynamic.set("compare-capture-alias", Value::Bool(false))?;

    app.set_capture_refractive(true);
    app.set_glass_depth(2.0);
    app.set_share_depth(false);
    dynamic.set("capture-refractive", Value::Bool(true))?;
    dynamic.set("glass-depth", Value::Float(2.0))?;
    dynamic.set("share-depth", Value::Bool(false))?;
    let (_, transmission) = draw()?;
    assert!(transmission[4] > split[4] && transmission[5] > split[5]);
    app.set_capture_color_format("r16f".into());
    dynamic.set("capture-color-format", Value::String("r16f".into()))?;
    let (reduced, refracted) = draw()?;
    assert_eq!(reduced[0][1..], [0, 0]);
    assert_eq!(refracted[1], transmission[1] + PIXELS * 2);
    assert_eq!(refracted[2], transmission[2]);
    assert!(refracted[4] > transmission[4] && refracted[5] > transmission[5]);
    assert_eq!(draw()?.1, refracted);
    Ok(())
}

#[test]
fn paired_capture_shares_the_opaque_draw_and_preserves_its_attachments() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    const PIXELS: u64 = 480 * 288;
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_paired_capture(true);
    dynamic.set("offscreen-composition", Value::Bool(true))?;
    dynamic.set("paired-capture", Value::Bool(true))?;
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw_number = 0;
    let mut draw = || -> TestResult<(Vec<u8>, [u64; 5])> {
        draw_number += 1;
        let png = aot.render(&mut gpu)?;
        assert!(
            png == interpreted.render(&mut other_gpu)?,
            "backend pixels differ at draw {draw_number}"
        );
        let image = xrack_assets::decode_png(png)?;
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_mrt_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.scene_capture_geometry_draws,
            ]
        };
        let work = counts(&gpu);
        assert_eq!(work, counts(&other_gpu));
        Ok((image.rgba[4 * 480 * 50..4 * 480 * 245].to_vec(), work))
    };

    let (separate, first) = draw()?;
    assert_eq!(first[..3], [2, 0, PIXELS * 20], "{first:?}");
    assert!(first[4] > 0);
    let sample = |pixels: &[u8], x: usize, y: usize| -> [u8; 3] {
        std::array::from_fn(|channel| pixels[4 * (480 * (y - 50) + x) + channel])
    };
    assert_ne!(sample(&separate, 180, 144), sample(&separate, 300, 144));
    let mut app = weak.upgrade().ok_or("missing paired capture fixture")?;
    app.set_combine_capture_attachments(true);
    dynamic.set("combine-capture-attachments", Value::Bool(true))?;
    let (paired, second) = draw()?;
    assert!(
        paired == separate,
        "MRT preserves lit color and depth/normal data"
    );
    assert_eq!(
        second,
        [
            first[0] + 1,
            1,
            first[2] + PIXELS * 20,
            first[3],
            first[4] * 2
        ]
    );
    assert_eq!(draw()?.1, second, "retained joint capture needs no redraw");

    app.set_paired_all_attachments(true);
    dynamic.set("paired-all-attachments", Value::Bool(true))?;
    let (over_budget, third) = draw()?;
    assert_ne!(sample(&over_budget, 300, 144), sample(&paired, 300, 144));
    assert_eq!(
        third[0..2],
        [second[0] + 1, 1],
        "wide HDR uses separate passes"
    );
    assert_eq!(third[2], second[2] + PIXELS * 16);
    assert_eq!(third[3], first[3]);

    app.set_paired_capture_format("r8".into());
    dynamic.set("paired-capture-format", Value::String("r8".into()))?;
    let (packed, fourth) = draw()?;
    assert_eq!(sample(&packed, 180, 144)[1..], [0, 0]);
    assert_eq!(sample(&packed, 300, 144), sample(&over_budget, 300, 144));
    assert_eq!(
        fourth[0..2],
        [third[0] + 1, 2],
        "packed color fits all attachments"
    );
    assert_eq!(fourth[2], third[2] + PIXELS * 17);
    assert_eq!(fourth[3], first[3]);
    assert_eq!(draw()?.1, fourth);

    app.set_combine_capture_attachments(false);
    dynamic.set("combine-capture-attachments", Value::Bool(false))?;
    assert!(
        draw()?.0 == packed,
        "all four material attachments match separate draws"
    );
    Ok(())
}

#[test]
fn material_data_capture_is_demand_driven_and_available_to_both_shader_interfaces() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    const PIXELS: u64 = 480 * 288;
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let dynamic = program.create("CaptureCompositeDemo")?;
    let mut app = CaptureCompositeDemo::new();
    app.set_offscreen_composition(true);
    app.set_paired_capture(true);
    app.set_combine_capture_attachments(true);
    app.set_data_sample_input(true);
    for field in [
        "offscreen-composition",
        "paired-capture",
        "combine-capture-attachments",
        "data-sample-input",
    ] {
        dynamic.set(field, Value::Bool(true))?;
    }
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut interpreted = PreviewScene::new(Box::new(dynamic.clone()));
    let renderer = || {
        pollster::block_on(Renderer::new(
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
            None,
        ))
    };
    let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
    let mut draw_number = 0;
    let mut draw = || -> TestResult<([[u8; 3]; 2], [u64; 5])> {
        draw_number += 1;
        let image = aot.render(&mut gpu)?;
        assert!(
            image == interpreted.render(&mut other_gpu)?,
            "draw {draw_number}"
        );
        let image = xrack_assets::decode_png(image)?;
        let samples = [180, 300].map(|x| {
            let start = 4 * (144 * 480 + x);
            std::array::from_fn(|channel| image.rgba[start + channel])
        });
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_mrt_passes,
                stats.scene_capture_bytes_allocated,
                stats.mesh_vertex_bytes_uploaded,
                stats.custom_material_pipeline_builds,
            ]
        };
        let work = counts(&gpu);
        assert_eq!(work, counts(&other_gpu));
        Ok((samples, work))
    };

    let (normals, unused) = draw()?;
    assert_eq!(
        unused[..3],
        [1, 1, PIXELS * 20],
        "no attachment or upstream pass for unused data"
    );
    assert_eq!(
        unused[4], 0,
        "ordinary shading does not compile a data-only shader"
    );
    let mut app = weak
        .upgrade()
        .ok_or("missing material data capture fixture")?;
    app.set_data_sample_input(false);
    app.set_paired_material_data(true);
    dynamic.set("data-sample-input", Value::Bool(false))?;
    dynamic.set("paired-material-data", Value::Bool(true))?;
    let (data, active) = draw()?;
    assert_eq!(data[0], normals[0]);
    assert!(
        data[1][0] > 120 && data[1][0] < 150 && data[1][2] > 160,
        "{data:?}"
    );
    assert_eq!(
        active,
        [2, 2, unused[2] + PIXELS * 8, unused[3], unused[4] + 1],
        "MRT reuses the color/depth textures and uploads no mesh data"
    );
    assert_eq!(draw()?.1, active);

    app.set_data_gain(0.6);
    dynamic.set("data-gain", Value::Float(0.6))?;
    let (changed, updated) = draw()?;
    assert!(changed[1][0] > data[1][0] + 50);
    assert_eq!(changed[0], normals[0]);
    assert_eq!(updated[2..], active[2..]);
    app.set_paired_data_format("r8".into());
    dynamic.set("paired-data-format", Value::String("r8".into()))?;
    let (packed, resized) = draw()?;
    assert_eq!(packed[0], normals[0]);
    assert_eq!(packed[1][1..], [0, 0]);
    assert!(packed[1][0].abs_diff(changed[1][0]) <= 2);
    assert_eq!(resized[2], updated[2] + PIXELS);

    app.set_combine_capture_attachments(false);
    dynamic.set("combine-capture-attachments", Value::Bool(false))?;
    let (separate, split) = draw()?;
    assert_eq!(separate, packed);
    assert_eq!(split[0..2], [resized[0] + 3, resized[1]]);
    app.set_data_sample_input(true);
    app.set_paired_data_format("rgba16f".into());
    dynamic.set("data-sample-input", Value::Bool(true))?;
    dynamic.set("paired-data-format", Value::String("rgba16f".into()))?;
    let (from_texture, sourced) = draw()?;
    assert!(
        from_texture[1][1] > changed[1][1] + 30,
        "{from_texture:?} {changed:?}"
    );
    assert!(
        sourced[0] >= split[0] + 2,
        "data hook requests its upstream capture"
    );

    app.set_paired_material_consumer(true);
    dynamic.set("paired-material-consumer", Value::Bool(true))?;
    let (material, _) = draw()?;
    assert!(material[0][0].abs_diff(changed[1][0]) <= 2, "{material:?}");
    assert!(
        material[0][1].abs_diff(from_texture[1][1]) <= 2,
        "{material:?}"
    );
    assert!(material[0][0] > normals[0][0] + 100);
    Ok(())
}

#[test]
fn capture_mrt_schedules_upstream_reads_without_merging_dependent_outputs() -> TestResult {
    use capture_composite_ui::CaptureCompositeDemo;

    const NAME: &str = "examples/puzzle3d/capture_composite.xr";
    let preview =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(NAME)?, NAME, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&preview.module, NAME, None)?;
    let render = |combine: bool,
                  background: &str,
                  source: &str,
                  mask: bool|
     -> TestResult<(Vec<u8>, [u64; 5])> {
        let dynamic = program.create("CaptureCompositeDemo")?;
        let mut app = CaptureCompositeDemo::new();
        app.set_offscreen_composition(true);
        app.set_paired_capture(true);
        app.set_paired_material_data(true);
        app.set_data_sample_input(true);
        app.set_combine_capture_attachments(combine);
        app.set_paired_background_capture(background.into());
        app.set_data_input_capture(source.into());
        app.set_paired_stencil(mask);
        for (name, value) in [
            ("offscreen-composition", true),
            ("paired-capture", true),
            ("paired-material-data", true),
            ("data-sample-input", true),
            ("combine-capture-attachments", combine),
            ("paired-stencil", mask),
        ] {
            dynamic.set(name, Value::Bool(value))?;
        }
        dynamic.set(
            "paired-background-capture",
            Value::String(background.into()),
        )?;
        dynamic.set("data-input-capture", Value::String(source.into()))?;
        let mut aot = PreviewScene::new(Box::new(app));
        let mut interpreted = PreviewScene::new(Box::new(dynamic));
        let renderer = || {
            pollster::block_on(Renderer::new(
                wgpu::Instance::new(
                    wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
                ),
                None,
            ))
        };
        let (mut gpu, mut other_gpu) = (renderer()?, renderer()?);
        let image = aot.render(&mut gpu)?.to_vec();
        assert!(
            image == interpreted.render(&mut other_gpu)?,
            "backend output for {combine} {background} {source} {mask}"
        );
        let counts = |renderer: &Renderer| {
            let stats = renderer.scene_draw_stats();
            [
                stats.scene_capture_passes,
                stats.scene_capture_mrt_passes,
                stats.scene_capture_composite_draws,
                stats.scene_capture_feedback_edges,
                stats.scene_capture_stencil_copies,
            ]
        };
        let work = counts(&gpu);
        assert_eq!(work, counts(&other_gpu));
        assert!(image == aot.render(&mut gpu)?);
        assert_eq!(work, counts(&gpu), "published captures are reusable");
        let image = xrack_assets::decode_png(&image)?;
        Ok((image.rgba[4 * 480 * 50..4 * 480 * 245].to_vec(), work))
    };
    let sample = |image: &[u8], x: usize, y: usize, channel: usize| {
        image[4 * (480 * (y - 50) + x) + channel]
    };
    for (background, source, mask, split, joined, feedback, seeded) in [
        ("", "accent", false, 4, 2, 0, 1),
        ("accent", "accent", false, 4, 2, 0, 2),
        ("lighting", "accent", false, 5, 4, 0, 2),
        ("lighting", "lighting", false, 4, 3, 2, 2),
        ("accent", "accent", true, 5, 3, 0, 2),
    ] {
        let (separate, a) = render(false, background, source, mask)?;
        let (combined, b) = render(true, background, source, mask)?;
        assert!(
            separate == combined,
            "visible result for {background} {source} {mask}"
        );
        assert_eq!(a, [split, 0, seeded, feedback, if mask { 3 } else { 0 }]);
        assert_eq!(b, [joined, 1, seeded, feedback, u64::from(mask)]);
        if background == "accent" {
            assert!(
                sample(&combined, 50, 144, 1) > 180,
                "uncovered color inherits the source"
            );
        }
        if !mask && source == "accent" {
            assert!(
                sample(&combined, 300, 144, 1) > 200,
                "data hook sees its source"
            );
        }
    }
    Ok(())
}
