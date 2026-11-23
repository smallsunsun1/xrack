fn main() -> Result<(), String> {
    xrack_platform_winit::run(off_axis_camera_ui::OffAxisCameraDemo::new())
}
