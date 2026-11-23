fn main() -> Result<(), String> {
    xrack_platform_winit::run(live_reflections_ui::LiveReflectionsDemo::new())
}
