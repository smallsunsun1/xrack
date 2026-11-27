fn main() -> Result<(), String> {
    match std::env::args_os()
        .skip(1)
        .find(|arg| {
            matches!(
                arg.to_str(),
                Some("--waves" | "--appearance" | "--sorting" | "--shape")
            )
        })
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
    {
        Some("--waves") => {
            xrack_platform_winit::run(particle_affectors_ui::ParticleWavesDemo::new())
        }
        Some("--appearance") => {
            xrack_platform_winit::run(particle_affectors_ui::ParticleAppearanceDemo::new())
        }
        Some("--sorting") => {
            xrack_platform_winit::run(particle_affectors_ui::ParticleOrderDemo::new())
        }
        Some("--shape") => {
            xrack_platform_winit::run(particle_affectors_ui::ParticleShapeAttractorDemo::new())
        }
        _ => xrack_platform_winit::run(particle_affectors_ui::ParticleAffectorsDemo::new()),
    }
}
