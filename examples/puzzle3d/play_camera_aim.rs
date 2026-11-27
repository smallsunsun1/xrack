fn main() -> Result<(), String> {
    if std::env::args_os().skip(1).any(|arg| arg == "--look-at") {
        return xrack_platform_winit::run(camera_aim_ui::LookAtDemo::new());
    }
    xrack_platform_winit::run(camera_aim_ui::CameraAimDemo::new())
}
