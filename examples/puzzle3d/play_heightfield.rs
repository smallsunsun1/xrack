fn main() -> Result<(), String> {
    xrack_platform_winit::run(heightfield_ui::HeightfieldDemo::new())
}
