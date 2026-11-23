fn main() -> Result<(), String> {
    xrack_platform_winit::run(texture_mips_ui::TextureMipmapDemo::new())
}
