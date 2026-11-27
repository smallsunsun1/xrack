fn main() -> Result<(), String> {
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--sprite-lights")
    {
        xrack_platform_winit::run(particle_lighting_ui::SpriteLightDemo::new())
    } else {
        xrack_platform_winit::run(particle_lighting_ui::ParticleLightingDemo::new())
    }
}
