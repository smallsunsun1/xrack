fn main() -> Result<(), String> {
    if std::env::args_os().skip(1).any(|arg| arg == "--waves") {
        xrack_platform_winit::run(particle_affectors_ui::ParticleWavesDemo::new())
    } else {
        xrack_platform_winit::run(particle_affectors_ui::ParticleAffectorsDemo::new())
    }
}
