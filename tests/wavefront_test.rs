use std::path::{Path, PathBuf};

use wavefront_ui::WavefrontDemo;
use xrack_renderer_femtovg::{RenderDevice, Renderer, preview::PreviewScene};
use xrack_runtime::{Value, scene3d::MeshAsset};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const SCENE: &str = "examples/puzzle3d/wavefront.xr";
const INPUTS: &[&str] = &[
    "examples/puzzle3d/models/concave-panel.obj",
    "examples/puzzle3d/models/wavefront/panel.mtl",
    "examples/puzzle3d/models/focus-grid.png",
];

#[test]
fn resolves_transitive_inputs_and_preserves_material_slots_through_the_import_pipeline()
-> TestResult {
    let text = std::fs::read_to_string(SCENE)?.replace(
        r#"@mesh-url("models/concave-panel.obj")"#,
        r#"@mesh-url("models/concave-panel.obj", lightmap-uv, auto-lod)"#,
    );
    let source = Path::new(SCENE);
    let prepared = xrack_compiler::prepare_preview(&text, SCENE, Default::default())?;
    let files: Vec<PathBuf> = INPUTS.iter().map(PathBuf::from).collect();
    for (missing, name) in [(1, "panel.mtl"), (2, "focus-grid.png")] {
        let mut partial = files.clone();
        partial.remove(missing);
        let error = xrack_resources::resolve_assets(&prepared.module, source, Some(&partial))
            .err()
            .ok_or("missing input must fail resolution")?;
        assert!(
            error.contains(name) && error.contains("assets ="),
            "{error}"
        );
    }
    let assets = xrack_resources::resolve_assets(&prepared.module, source, Some(&files))?;
    for file in files {
        assert!(assets.inputs.contains(&file.canonicalize()?));
    }
    let mesh = assets.scene.meshes.values().next().ok_or("missing OBJ")?;
    let MeshAsset::Static(mesh) = mesh else {
        return Err("expected static OBJ mesh".into());
    };
    assert_eq!(mesh.vertices().len(), 24);
    assert_eq!(mesh.lightmap_uvs().len(), mesh.vertices().len());
    let slots: Vec<_> = mesh
        .material_slots()
        .iter()
        .map(|slot| (slot.source_index, slot.name.as_deref()))
        .collect();
    assert_eq!(
        slots,
        [(Some(0), Some("Backdrop")), (Some(1), Some("Trim"))]
    );
    Ok(())
}

#[test]
fn preview_and_aot_render_and_pick_concave_obj_and_reuse_geometry() -> TestResult {
    let source = std::fs::read_to_string(SCENE)?;
    let prepared = xrack_compiler::prepare_preview(&source, SCENE, Default::default())?;
    let (program, inputs) =
        xrack_interpreter::Program::from_module_with_file_resources(&prepared.module, SCENE, None)?;
    for file in INPUTS {
        assert!(inputs.contains(&Path::new(file).canonicalize()?));
    }
    let interpreted = program.create("WavefrontDemo")?;
    let app = WavefrontDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut preview = PreviewScene::new(Box::new(interpreted.clone()));
    let device = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut aot_gpu = Renderer::with_device(device.clone())?;
    let mut preview_gpu = Renderer::with_device(device)?;
    let mut app = weak.upgrade().ok_or("missing OBJ demo")?;
    for (x, z) in [(0.0, 0.0), (1.25, 1.0)] {
        app.set_ray_x(x);
        interpreted.set("ray-x", Value::Float(x))?;
        let hit = app.get_ray_hit();
        assert!(hit.valid && hit.object_id == "panel");
        assert!((hit.position.z - z).abs() < 1e-6);
        assert_eq!(interpreted.get("ray-hit")?, Value::Hit3D(hit));
    }
    let original = aot.render(&mut aot_gpu)?.to_vec();
    assert!(original == preview.render(&mut preview_gpu)?);
    let uploads = [
        aot_gpu.scene_mesh_uploads(),
        preview_gpu.scene_mesh_uploads(),
    ];
    app.set_painted(true);
    interpreted.set("painted", Value::Bool(true))?;
    let updated = aot.render(&mut aot_gpu)?;
    assert_ne!(original, updated);
    assert!(updated == preview.render(&mut preview_gpu)?);
    assert_eq!(
        uploads,
        [
            aot_gpu.scene_mesh_uploads(),
            preview_gpu.scene_mesh_uploads()
        ]
    );
    Ok(())
}

#[test]
fn host_loaded_mesh_round_trips_and_reuses_gpu_geometry_in_both_backends() -> TestResult {
    use runtime_mesh_ui::RuntimeMeshDemo;
    use xrack_runtime::MeshAsset3D;

    const SOURCE: &str = "examples/puzzle3d/runtime_mesh.xr";
    let text = std::fs::read_to_string(SOURCE)?;
    let staged = xrack_compiler::prepare_preview(&text, SOURCE, Default::default())?;
    let interpreted =
        xrack_interpreter::Program::from_module(&staged.module)?.create("RuntimeMeshDemo")?;
    let app = RuntimeMeshDemo::new();
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut preview = PreviewScene::new(Box::new(interpreted.clone()));
    let device = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut aot_gpu = Renderer::with_device(device.clone())?;
    let mut preview_gpu = Renderer::with_device(device)?;
    let fallback = aot.render(&mut aot_gpu)?.to_vec();
    assert!(fallback == preview.render(&mut preview_gpu)?);
    let mut app = weak.upgrade().ok_or("missing runtime loading demo")?;
    assert!(!app.get_hit().valid);
    assert!(!app.get_loaded().is_loaded());

    let file =
        std::thread::spawn(|| xrack_scene_import::load_mesh_file(INPUTS[0], Default::default()))
            .join()
            .map_err(|_| "mesh preparation panicked")??;
    assert!(file.asset.is_loaded());
    assert_eq!(file.inputs.len(), INPUTS.len());
    for path in INPUTS {
        assert!(file.inputs.contains(&Path::new(path).canonicalize()?));
    }
    interpreted.set("loaded", file.asset.clone().into())?;
    app.set_loaded(file.asset.clone());
    let asset = Value::MeshAsset3D(file.asset);
    assert_eq!(interpreted.get("loaded")?, asset);

    // The edited schema uses only the injected mesh. Keep the prepared asset
    // when reloading the source without the placeholder and its string field.
    let edited = text
        .replace("mesh: string, ", "")
        .replace("mesh: \"primitive:box\", ", "");
    let edited = xrack_compiler::prepare_preview(&edited, SOURCE, Default::default())?;
    let live = xrack_interpreter::ReloadableInstance::new(interpreted.clone());
    live.reload(&xrack_interpreter::Program::from_module(&edited.module)?)?;
    assert_eq!(live.current().get("loaded")?, asset);
    let hit = app.get_hit();
    assert!(hit.valid && hit.object_id == "object" && hit.position.z.abs() < 1e-6);
    assert_eq!(interpreted.get("hit")?, Value::Hit3D(hit));
    let imported = aot.render(&mut aot_gpu)?.to_vec();
    assert_ne!(fallback, imported);
    assert!(imported == preview.render(&mut preview_gpu)?);
    let uploads = [
        aot_gpu.scene_mesh_uploads(),
        preview_gpu.scene_mesh_uploads(),
    ];

    app.set_turn(40.0);
    interpreted.set("turn", Value::Float(40.0))?;
    let rotated = aot.render(&mut aot_gpu)?;
    assert_ne!(imported, rotated);
    assert!(rotated == preview.render(&mut preview_gpu)?);
    assert_eq!(
        uploads,
        [
            aot_gpu.scene_mesh_uploads(),
            preview_gpu.scene_mesh_uploads()
        ]
    );

    app.set_loaded(MeshAsset3D::default());
    interpreted.set("loaded", MeshAsset3D::default().into())?;
    app.set_turn(0.0);
    interpreted.set("turn", Value::Float(0.0))?;
    assert!(!app.get_hit().valid);
    assert_eq!(fallback, aot.render(&mut aot_gpu)?);
    assert_eq!(fallback, preview.render(&mut preview_gpu)?);
    Ok(())
}

#[test]
fn declarative_loader_updates_the_scene_and_ignores_replaced_requests() -> TestResult {
    use declarative_mesh_ui::DeclarativeMeshDemo;
    use std::time::{Duration, Instant};

    const SOURCE: &str = "examples/puzzle3d/declarative_mesh.xr";
    let module = xrack_compiler::prepare_preview(
        &std::fs::read_to_string(SOURCE)?,
        SOURCE,
        Default::default(),
    )?;
    let program = xrack_interpreter::Program::from_module(&module.module)?;
    let interpreted = program.create("DeclarativeMeshDemo")?;
    let app = DeclarativeMeshDemo::new();
    assert!(!app.get_projected_center().valid);
    assert!(!app.get_view_hit().valid);
    let weak = app.as_weak();
    let mut aot = PreviewScene::new(Box::new(app));
    let mut preview = PreviewScene::new(Box::new(interpreted.clone()));
    let device = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut aot_gpu = Renderer::with_device(device.clone())?;
    let mut preview_gpu = Renderer::with_device(device)?;
    let app = weak.upgrade().ok_or("missing declarative mesh demo")?;

    let mut frame = |status: &str| -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
        let timeout = Instant::now() + Duration::from_secs(5);
        loop {
            let compiled = aot.render(&mut aot_gpu)?.to_vec();
            let compiled_status = app.get_status();
            let dynamic = preview.render(&mut preview_gpu)?.to_vec();
            if compiled_status == status
                && interpreted.get("status")? == Value::String(status.into())
            {
                return Ok((compiled, dynamic));
            }
            if Instant::now() >= timeout {
                return Err(format!(
                    "expected {status}, got AOT {:?}, interpreter {:?}",
                    app.get_status(),
                    interpreted.get("status")?
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    };
    let (empty, dynamic) = frame("empty")?;
    assert_eq!(empty, dynamic);

    // Enqueue a failure, then change the source before delivering the result.
    // The later scene must publish the real mesh and its successful query.
    let mut write = weak.upgrade().ok_or("missing runtime demo")?;
    write.set_path("models/missing.obj".into());
    interpreted.set("path", Value::String("models/missing.obj".into()))?;
    let _ = write.render();
    interpreted.try_render()?;
    assert_eq!(app.get_status(), "loading");
    assert_eq!(interpreted.get("status")?, Value::String("loading".into()));
    write.set_path("models/concave-panel.obj".into());
    interpreted.set("path", Value::String("models/concave-panel.obj".into()))?;
    let _ = write.render();
    interpreted.try_render()?;
    let (loaded, dynamic) = frame("ready")?;
    assert_ne!(loaded, empty);
    assert_eq!(loaded, dynamic);
    assert!(app.get_loaded().is_loaded());
    assert!(app.get_error().is_empty());
    let asset_bounds = app.get_asset_bounds();
    assert_eq!(
        asset_bounds.min(),
        xrack_runtime::Point3D::new(-2.0, -1.0, 0.0)
    );
    assert_eq!(
        asset_bounds.max(),
        xrack_runtime::Point3D::new(2.0, 1.0, 1.0)
    );
    assert_eq!(app.get_loaded().bounds(), asset_bounds);
    assert_eq!(
        interpreted.get("asset-bounds")?,
        Value::Bounds3D(asset_bounds)
    );
    assert_eq!(
        interpreted.get("world-bounds")?,
        Value::Bounds3D(app.get_world_bounds())
    );

    let hit = app.get_hit();
    assert!(hit.valid && hit.position.z.abs() < 1e-6);
    assert_eq!(interpreted.get("hit")?, Value::Hit3D(hit));
    let restored = program.create("DeclarativeMeshDemo")?;
    restored.restore_preview_state(&interpreted)?;
    restored.try_render()?;
    assert_eq!(restored.get("loaded")?, interpreted.get("loaded")?);
    assert_eq!(restored.get("status")?, Value::String("ready".into()));

    write.set_path("models/missing.obj".into());
    interpreted.set("path", Value::String("models/missing.obj".into()))?;
    let (failed, dynamic) = frame("error")?;
    assert_eq!(failed, dynamic);
    assert!(app.get_error().contains("missing.obj"));
    assert!(!app.get_loaded().is_loaded());
    write.set_active(false);
    interpreted.set("active", Value::Bool(false))?;
    let (disabled, dynamic) = frame("empty")?;
    assert!(!app.get_loaded().is_loaded());
    assert_eq!(disabled, dynamic);
    assert!(!app.get_asset_bounds().valid());

    let directory =
        std::env::var_os("TEST_TMPDIR").ok_or("missing Bazel test scratch directory")?;
    let remote_mesh = Path::new(&directory).join("offset-panel.obj");
    std::fs::write(
        &remote_mesh,
        "v 3000 -100 0\nv 3400 -100 0\nv 3400 100 0\nv 3000 100 0\nf 1 2 3 4\n",
    )?;
    write.set_active(true);
    interpreted.set("active", Value::Bool(true))?;
    write.set_path(remote_mesh.to_string_lossy().into_owned());
    interpreted.set(
        "path",
        Value::String(remote_mesh.to_string_lossy().into_owned()),
    )?;
    let (framed, dynamic) = frame("ready")?;
    assert_eq!(framed, dynamic);
    let world_bounds = app.get_world_bounds();
    assert!((world_bounds.center().x - 3200.0).abs() < 0.1);
    let scene = app.render();
    let view = scene
        .materialized_children()
        .find_map(|(_, node)| (node.kind == "SceneView3D").then_some(node))
        .ok_or("missing scene view")?;
    let camera = xrack_runtime::scene3d::SceneCamera::for_view(view, [540.0, 244.0])
        .ok_or("invalid fitted camera")?;
    assert!(camera.project(world_bounds.center()).inside);
    let projected = app.get_projected_center();
    assert_eq!(projected, camera.project(world_bounds.center()));
    assert_eq!(
        interpreted.get("projected-center")?,
        Value::ProjectedPoint3D(projected)
    );
    assert!(projected.valid && projected.inside);
    assert!((app.get_round_trip().x - 3200.0).abs() < 0.1);
    assert_eq!(
        interpreted.get("round-trip")?,
        Value::Point3D(app.get_round_trip())
    );
    let view_hit = app.get_view_hit();
    assert!(view_hit.valid && (view_hit.position.x - 3200.0).abs() < 0.1);
    assert_eq!(interpreted.get("view-hit")?, Value::Hit3D(view_hit.clone()));
    assert_eq!(app.get_view_row_hit(), view_hit);
    assert_eq!(app.get_view_hits(), [view_hit.clone()]);
    assert_eq!(app.get_view_row_hits(), [view_hit]);
    assert_eq!(app.get_frame_rows(), [0]);

    write.set_viewport_width(xrack_runtime::Length(328.0));
    interpreted.set(
        "viewport-width",
        Value::Length(xrack_runtime::Length(328.0)),
    )?;
    let resized = aot.render(&mut aot_gpu)?;
    assert_eq!(resized, preview.render(&mut preview_gpu)?);
    let projected = app.get_projected_center();
    assert!((projected.point.x.0 - 164.0).abs() < 0.05);
    assert_eq!(
        interpreted.get("projected-center")?,
        Value::ProjectedPoint3D(projected)
    );

    let pointer = xrack_layout::Point { x: 164.0, y: 122.0 };
    let contact = xrack_renderer_femtovg::preview::PointerContact {
        id: 1,
        primary: true,
        event: xrack_runtime::PointerEvent {
            button: xrack_runtime::PointerButton::Primary,
            ..Default::default()
        },
    };
    for scene in [&mut aot, &mut preview] {
        scene.press_contact(contact, pointer, false);
        scene.finish_contact(contact, pointer, true, false);
    }
    aot.render(&mut aot_gpu)?;
    preview.render(&mut preview_gpu)?;
    assert_eq!(app.get_clicked_object(), "object");
    assert_eq!(
        interpreted.get("clicked-object")?,
        Value::String("object".into())
    );

    let uploads = [
        aot_gpu.scene_mesh_uploads(),
        preview_gpu.scene_mesh_uploads(),
    ];
    write.set_auto_frame(false);
    interpreted.set("auto-frame", Value::Bool(false))?;
    let unfitted = aot.render(&mut aot_gpu)?;
    assert_eq!(unfitted, preview.render(&mut preview_gpu)?);
    assert_ne!(unfitted, framed);
    assert!(!app.get_view_hit().valid);
    assert_eq!(
        interpreted.get("view-hit")?,
        Value::Hit3D(app.get_view_hit())
    );
    assert_eq!(
        uploads,
        [
            aot_gpu.scene_mesh_uploads(),
            preview_gpu.scene_mesh_uploads()
        ]
    );
    Ok(())
}

#[test]
fn gltf_node_queries_preserve_names_indices_and_transforms_in_both_backends() -> TestResult {
    use mesh_nodes_ui::MeshNodeDemo;
    use std::time::{Duration, Instant};
    use xrack_runtime::Point3D;

    const PATH: &str = "examples/puzzle3d/mesh_nodes.xr";
    let program =
        xrack_compiler::prepare_preview(&std::fs::read_to_string(PATH)?, PATH, Default::default())?;
    let (program, _) =
        xrack_interpreter::Program::from_module_with_file_resources(&program.module, PATH, None)?;
    let dynamic = program.create("MeshNodeDemo")?;
    let app = MeshNodeDemo::new();
    let weak = app.as_weak();
    let mut compiled = PreviewScene::new(Box::new(app));
    let mut preview = PreviewScene::new(Box::new(dynamic.clone()));
    let device = pollster::block_on(RenderDevice::request(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env()),
        None,
    ))?;
    let mut first = Renderer::with_device(device.clone())?;
    let mut second = Renderer::with_device(device)?;
    let mut app = weak.upgrade().ok_or("missing query demo")?;

    let wait = |app: &MeshNodeDemo,
                compiled: &mut PreviewScene,
                preview: &mut PreviewScene,
                first: &mut Renderer,
                second: &mut Renderer|
     -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let timeout = Instant::now() + Duration::from_secs(5);
        loop {
            let aot = compiled.render(first)?.to_vec();
            let status = app.get_status();
            let interpreted = preview.render(second)?;
            if status == "ready" && dynamic.get("status")? == Value::String("ready".into()) {
                assert!(aot == interpreted);
                return Ok(aot);
            }
            if Instant::now() >= timeout {
                return Err(format!(
                    "unable to load node query demo: {:?}",
                    dynamic.get("status")?
                )
                .into());
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    };

    let first_pose = wait(&app, &mut compiled, &mut preview, &mut first, &mut second)?;
    let pivot = app.get_selected();
    assert!(pivot.valid() && !pivot.has_mesh);
    assert_eq!(pivot.name, "Pivot");
    assert_eq!(pivot.path, "/Workshop/Door~1Leaf/Pivot");
    assert_eq!((pivot.source_index, pivot.parent_index), (2, 1));
    assert_eq!(pivot.origin, Point3D::new(5.5, 1.5, 0.0));
    assert_eq!(app.get_compiled(), pivot);
    assert_eq!(app.get_posed(), pivot);
    assert_eq!(dynamic.get("posed")?, Value::MeshNode3D(pivot.clone()));
    assert_eq!(dynamic.get("selected")?, Value::MeshNode3D(pivot.clone()));
    assert_eq!(dynamic.get("compiled")?, Value::MeshNode3D(pivot));
    assert_eq!(app.get_indices(), [0, 1, 2, 3, 4, 5]);
    assert_eq!(app.get_compiled_meshes(), [3]);
    let rig = app.get_compiled_rig();
    assert!(rig.valid() && rig.has_mesh && rig.path == "/#3/Rear table");
    assert_eq!(rig.source_index, 0);
    assert!((rig.origin.z + 1.3).abs() < 1e-5);
    assert_eq!(dynamic.get("compiled-rig")?, Value::MeshNode3D(rig.clone()));

    for selector in ["Pivot", "#6", "/Workshop/Cabinet/Pivot", "#2"] {
        app.set_selector(selector.into());
        dynamic.set("selector", Value::String(selector.into()))?;
        let aot = compiled.render(&mut first)?.to_vec();
        assert!(aot == preview.render(&mut second)?);
        assert_eq!(
            dynamic.get("selected")?,
            Value::MeshNode3D(app.get_selected())
        );
        assert_eq!(app.get_posed(), app.get_selected());
        assert_eq!(dynamic.get("posed")?, Value::MeshNode3D(app.get_posed()));
        if selector == "/Workshop/Cabinet/Pivot" {
            assert_eq!(app.get_selected().source_index, 5);
            assert_ne!(aot, first_pose);
        } else if selector == "#2" {
            assert_eq!(app.get_selected().source_index, 2);
        } else {
            assert!(!app.get_selected().valid());
        }
    }

    app.set_path("models/instance-picking.gltf".into());
    app.set_selector("Rear table".into());
    app.set_kind("mesh".into());
    dynamic.set("path", Value::String("models/instance-picking.gltf".into()))?;
    dynamic.set("selector", Value::String("Rear table".into()))?;
    dynamic.set("kind", Value::String("mesh".into()))?;
    wait(&app, &mut compiled, &mut preview, &mut first, &mut second)?;
    assert_eq!(app.get_selected(), rig);
    assert_eq!(app.get_indices(), [2, 0, 4, 5]);
    assert_eq!(dynamic.get("selected")?, Value::MeshNode3D(rig));
    Ok(())
}
