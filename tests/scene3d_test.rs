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
    }
    let mut render = |frame: Frame| -> TestResult<_> {
        let mut aot = weak.upgrade().ok_or("missing transparency demo")?;
        aot.set_weighted(frame.weighted);
        aot.set_reversed(frame.reversed);
        aot.set_smooth(frame.smooth);
        aot.set_hdr(frame.hdr);
        aot.set_single(frame.single);
        aot.set_offscreen(frame.offscreen);
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
    let (sorted, sorted_counts) = render(base)?;
    let (sorted_reverse, _) = render(Frame {
        reversed: true,
        ..base
    })?;
    assert_eq!(sorted.size, [480, 300]);
    assert!(
        difference(&sorted, &sorted_reverse) > 20,
        "submesh ties follow submission order"
    );

    let enabled = Frame {
        weighted: true,
        ..base
    };
    let (weighted, batches) = render(enabled)?;
    let (weighted_reverse, reverse_batches) = render(Frame {
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
    let left = pixel(&weighted, 185, 98);
    let right = pixel(&weighted, 295, 98);
    assert!(
        left[0] > left[2] && right[2] > right[0],
        "per-pixel depth: {left:?} and {right:?}"
    );

    let (occluder_only, _) = render(Frame {
        offscreen: true,
        ..enabled
    })?;
    assert_eq!(pixel(&weighted, 240, 150), pixel(&occluder_only, 240, 150));
    let hd = Frame {
        smooth: true,
        hdr: true,
        ..enabled
    };
    let (hdr, _) = render(hd)?;
    let (hdr_reverse, _) = render(Frame {
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
        let (single_weighted, _) = render(single)?;
        let (single_sorted, _) = render(Frame {
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
fn sphere_walk_steps_snaps_and_respects_slope_and_headroom_in_both_backends() -> TestResult {
    use sphere_walk_ui::SphereWalkDemo;
    use xrack_runtime::scene3d::Point3D;

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
fn generated_lightmap_module_drives_aot_and_interpreter_without_live_shadows() -> TestResult {
    use baked_room_ui::BakedRoomDemo;

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
    let mut draw = |baked: bool| -> TestResult<(Vec<u8>, u64, u64)> {
        let mut app = weak.upgrade().ok_or("missing baked room")?;
        app.set_baked(baked);
        dynamic.set("baked", Value::Bool(baked))?;
        let shadow_passes = gpu.scene_shadow_passes();
        let baked_draws = gpu.scene_draw_stats().lightmapped_draws;
        let png = generated.render(&mut gpu)?.to_vec();
        assert!(png.as_slice() == interpreted.render(&mut other)?);
        let view = xrack_assets::decode_png(&png)?.rgba[..600 * 340 * 4].to_vec();
        Ok((
            view,
            gpu.scene_shadow_passes() - shadow_passes,
            gpu.scene_draw_stats().lightmapped_draws - baked_draws,
        ))
    };
    let (baked, shadow_passes, baked_draws) = draw(true)?;
    assert_eq!(shadow_passes, 0);
    assert!(
        baked_draws >= 2,
        "authored and generated charts both receive light"
    );
    let (live, shadow_passes, baked_draws) = draw(false)?;
    assert!(shadow_passes > 0);
    assert_eq!(baked_draws, 0);
    assert!(baked != live);
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
    let mut draw =
        |angle, scope, should_draw, history, pending| -> TestResult<xrack_assets::Raster> {
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
            Ok(xrack_assets::decode_png(&png)?)
        };
    let _ = draw(0.0, 1, true, false, true)?;
    for index in 1..8 {
        let _ = draw(0.0, 1, true, true, index < 7)?;
    }
    let _ = draw(0.0, 1, false, false, false)?;
    // A separate scope starts cold at the same subpixel phase. History must
    // contribute real color to the warm render, beyond just drawing vectors.
    let warm = draw(3.0, 1, true, true, true)?;
    let cold = draw(3.0, 2, true, false, true)?;
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

    let moved = draw(85.0, 1, true, true, true)?;
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
    assert_eq!(mirrored.mesh_uploads, rest.mesh_uploads + 2);
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
        ))
    };

    let (baseline, draws, compactions, instances) = frame(false, 0.0)?;
    assert_eq!(draws, [1, object_count, 0]);
    let (culled, draws, packed, reused_instances) = frame(true, 0.0)?;
    assert_eq!(culled, baseline);
    assert_eq!(draws, [1, 65, object_count - 65]);
    assert_eq!(packed, compactions + 1);
    assert_eq!(reused_instances, instances);
    let (moved, draws, after_move, reused_instances) = frame(true, 0.05)?;
    assert_ne!(moved, culled);
    assert_eq!(draws, [1, 65, object_count - 65]);
    assert_eq!(
        after_move, packed,
        "movement that keeps the same visible mask reuses the compact buffer"
    );
    assert_eq!(reused_instances, instances);
    let (empty, draws, _, _) = frame(true, -80.0)?;
    assert_eq!(draws, [0, 0, object_count]);
    assert_eq!(
        scene_pixel(&xrack_assets::decode_png(&empty)?, [100, 100]),
        [8, 9, 10, 255]
    );
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
        app.set_animated(view.animated);
        app.set_automatic(view.automatic);
        app.set_time(view.time);
        app.set_shadows(view.shadows);
        for (name, value) in [
            ("distance", Value::Float(view.distance)),
            ("orthographic", Value::Bool(view.orthographic)),
            ("full-detail", Value::Bool(view.full_detail)),
            ("object-bias", Value::Float(view.bias)),
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
    let body = r#"
        struct Bad { id: string }
        export component Test inherits Window {
            property <[Bad]> room: [];
            BODY
        }
    "#;
    for use_site in [
        "SceneView3D { model: root.room; }",
        "out property <string> hit: Scene3D.pick(root.room, 0, 0, 8, Point2D.new(200px, 200px), Point2D.new(100px, 100px));",
    ] {
        let source = body.replace("BODY", use_site);
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
    let complete = body.replace(
        "id: string",
        "id: string, mesh: string, x: float, y: float, z: float, scale-x: float, scale-y: float, scale-z: float, yaw: float, pitch: float, roll: float, color: color, parent: bool",
    ).replace("BODY", "SceneView3D { model: root.room; }");
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
    assert!(error.to_string().contains("field named mesh"), "{error}");
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
                    base_color: [1.0; 4],
                    alpha_mode,
                    double_sided: false,
                    base_texture: None,
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
                base_color: [1.0; 4],
                alpha_mode,
                double_sided: false,
                base_texture: None,
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
