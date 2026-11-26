fn main() -> Result<(), String> {
    match std::env::args_os()
        .skip(1)
        .find(|arg| matches!(arg.to_str(), Some("--birth-track" | "--impacts")))
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("--birth-track") => {
            xrack_platform_winit::run(particle_trails_ui::ParticleBirthTrackDemo::new())
        }
        Some("--impacts") => {
            xrack_platform_winit::run(particle_trails_ui::ParticleImpactDemo::new())
        }
        _ => xrack_platform_winit::run(particle_trails_ui::ParticleTrailsDemo::new()),
    }
}
