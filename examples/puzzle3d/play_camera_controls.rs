fn main() -> Result<(), String> {
    xrack_platform_winit::run(camera_controls_ui::CameraControlsDemo::new())
}
