fn main() -> Result<(), String> {
    xrack_platform_winit::run(focus_ui::FocusDemo::new())
}
