fn main() -> Result<(), String> {
    xrack_platform_winit::run(quality_ui::QualityDemo::new())
}
