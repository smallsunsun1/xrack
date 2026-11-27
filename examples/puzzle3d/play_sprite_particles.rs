fn main() -> Result<(), String> {
    match std::env::args_os()
        .skip(1)
        .find(|arg| {
            matches!(
                arg.to_str(),
                Some("--variation" | "--orientation" | "--soft" | "--pivot" | "--reverse-life")
            )
        })
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("--reverse-life") => {
            xrack_platform_winit::run(sprite_particles_ui::ParticleReversedLifeDemo::new())
        }
        Some("--variation") => {
            xrack_platform_winit::run(sprite_particles_ui::ParticleBirthVariationsDemo::new())
        }
        Some("--orientation") => {
            xrack_platform_winit::run(sprite_particles_ui::ParticleOrientationDemo::new())
        }
        Some("--soft") => xrack_platform_winit::run(sprite_particles_ui::SoftParticlesDemo::new()),
        Some("--pivot") => xrack_platform_winit::run(sprite_particles_ui::ParticlePivotDemo::new()),
        _ => xrack_platform_winit::run(sprite_particles_ui::SpriteParticlesDemo::new()),
    }
}
