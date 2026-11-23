fn main() -> Result<(), String> {
    xrack_platform_winit::run(dispersion_ui::DispersionDemo::new())
}
