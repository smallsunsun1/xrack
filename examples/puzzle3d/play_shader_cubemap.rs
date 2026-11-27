fn main() -> Result<(), String> {
    xrack_platform_winit::run(shader_cubemap_ui::ShaderCubemapDemo::new())
}
