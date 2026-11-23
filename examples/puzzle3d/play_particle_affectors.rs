fn main() -> Result<(), String> {
    xrack_platform_winit::run(particle_affectors_ui::ParticleAffectorsDemo::new())
}
