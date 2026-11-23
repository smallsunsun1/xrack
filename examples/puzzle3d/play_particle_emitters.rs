fn main() -> Result<(), String> {
    xrack_platform_winit::run(particle_emitters_ui::ParticleEmittersDemo::new())
}
