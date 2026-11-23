fn main() -> Result<(), String> {
    xrack_platform_winit::run(lightmaps_ui::LightmapsDemo::new())
}
