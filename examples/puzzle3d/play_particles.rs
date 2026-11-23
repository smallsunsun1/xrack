fn main() -> Result<(), String> {
    xrack_platform_winit::run(particles_ui::ParticleDemo::new())
}
