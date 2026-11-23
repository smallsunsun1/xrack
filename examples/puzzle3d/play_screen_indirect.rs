fn main() -> Result<(), String> {
    xrack_platform_winit::run(screen_indirect_ui::ScreenIndirectDemo::new())
}
