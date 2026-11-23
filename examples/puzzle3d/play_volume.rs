fn main() -> Result<(), String> {
    xrack_platform_winit::run(volume_ui::VolumeDemo::new())
}
