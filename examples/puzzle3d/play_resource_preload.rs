fn main() -> Result<(), String> {
    xrack_platform_winit::run(resource_preload_ui::ResourcePreloadDemo::new())
}
