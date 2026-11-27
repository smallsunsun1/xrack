fn main() -> Result<(), String> {
    if std::env::args_os().skip(1).any(|arg| arg == "--fov-axis") {
        return xrack_platform_winit::run(off_axis_camera_ui::FieldOfViewDemo::new());
    }
    xrack_platform_winit::run(off_axis_camera_ui::OffAxisCameraDemo::new())
}
