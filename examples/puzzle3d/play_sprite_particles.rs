fn main() -> Result<(), String> {
    if std::env::args_os().skip(1).any(|arg| arg == "--variation") {
        xrack_platform_winit::run(sprite_particles_ui::ParticleBirthVariationsDemo::new())
    } else {
        xrack_platform_winit::run(sprite_particles_ui::SpriteParticlesDemo::new())
    }
}
