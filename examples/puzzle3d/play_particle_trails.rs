fn main() -> Result<(), String> {
    xrack_platform_winit::run(particle_trails_ui::ParticleTrailsDemo::new())
}
