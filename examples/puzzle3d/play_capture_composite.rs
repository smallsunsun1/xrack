fn main() -> Result<(), String> {
    xrack_platform_winit::run(capture_composite_ui::CaptureCompositeDemo::new())
}
