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
