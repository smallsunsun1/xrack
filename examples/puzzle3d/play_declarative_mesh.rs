fn main() -> Result<(), String> {
    let mut demo = declarative_mesh_ui::DeclarativeMeshDemo::new();
    if let Some(path) = std::env::args_os().nth(1) {
        let path = std::path::absolute(path).map_err(|error| error.to_string())?;
        demo.set_path(path.to_string_lossy().into_owned());
    }
    xrack_platform_winit::run(demo)
}
