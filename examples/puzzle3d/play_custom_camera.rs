fn main() -> Result<(), String> {
    xrack_platform_winit::run(custom_camera_ui::CustomCameraDemo::new())
}
