fn main() -> Result<(), String> {
    xrack_platform_winit::run(scene_capture_ui::SceneCaptureDemo::new())
}
