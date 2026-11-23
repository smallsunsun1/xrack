fn main() -> Result<(), String> {
    xrack_platform_winit::run(transparency_ui::TransparencyDemo::new())
}
