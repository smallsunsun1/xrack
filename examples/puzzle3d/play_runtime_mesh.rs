use runtime_mesh_ui::RuntimeMeshDemo;
use std::path::{Path, PathBuf};
use xrack_scene_import::{MeshFile, load_mesh_file};

fn main() -> Result<(), String> {
    let mut app = RuntimeMeshDemo::new();
    if let Some(path) = std::env::args_os().nth(1) {
        let path = PathBuf::from(path);
        let result = load_mesh_file(&path, Default::default());
        apply_result(&mut app, &path, result);
    }

    let weak = app.as_weak();
    app.on_load_mesh(move |path| {
        let Some(mut app) = weak.upgrade() else {
            return;
        };
        if path.is_empty() || app.get_loading() {
            return;
        }
        app.set_loading(true);
        app.set_load_error(String::new());
        let weak = weak.clone();
        let worker = std::thread::Builder::new().name("xrack-mesh-import".into());
        if let Err(error) = worker.spawn(move || {
            let path = PathBuf::from(path);
            let result = load_mesh_file(&path, Default::default());
            let _ =
                weak.upgrade_in_event_loop(move |mut app| apply_result(&mut app, &path, result));
        }) {
            app.set_load_error(error.to_string());
            app.set_loading(false);
        }
    });
    xrack_platform_winit::run(app)
}

fn apply_result(app: &mut RuntimeMeshDemo, path: &Path, result: Result<MeshFile, String>) {
    match result {
        Ok(file) => {
            app.set_loaded(file.asset);
            app.set_source_name(path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ));
            app.set_load_error(String::new());
        }
        Err(error) => app.set_load_error(error),
    }
    app.set_loading(false);
}
