fn main() -> Result<(), String> {
    xrack_platform_winit::run(volume_texture_ui::VolumeTextureDemo::new())
}
