fn main() -> Result<(), String> {
    xrack_platform_winit::run(capture_augment_ui::CaptureAugmentDemo::new())
}
