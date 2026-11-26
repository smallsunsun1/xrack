fn main() -> Result<(), String> {
    xrack_platform_winit::run(capture_mipmaps_ui::CaptureMipmapsDemo::new())
}
