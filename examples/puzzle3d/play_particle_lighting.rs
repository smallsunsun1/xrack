fn main() -> Result<(), String> {
    xrack_platform_winit::run(particle_lighting_ui::ParticleLightingDemo::new())
}
