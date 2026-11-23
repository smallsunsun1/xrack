fn main() -> Result<(), String> {
    xrack_platform_winit::run(uv_atlas_ui::UvAtlasDemo::new())
}
