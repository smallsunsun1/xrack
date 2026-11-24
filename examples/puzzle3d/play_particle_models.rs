fn main() -> Result<(), String> {
    xrack_platform_winit::run(particle_models_ui::ParticleModelsDemo::new())
}
