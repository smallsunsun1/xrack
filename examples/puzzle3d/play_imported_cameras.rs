fn main() -> Result<(), String> {
    xrack_platform_winit::run(imported_cameras_ui::ImportedCamerasDemo::new())
}
