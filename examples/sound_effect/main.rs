fn main() -> Result<(), String> {
    xrack_platform_winit::run(sound_ui::SoundDemo::new())
}
