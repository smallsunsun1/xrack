fn main() -> Result<(), String> {
    if std::env::args_os().skip(1).any(|arg| arg == "--points") {
        xrack_platform_winit::run(particle_mesh_emitters_ui::ParticlePointCloudDemo::new())
    } else if std::env::args_os().skip(1).any(|arg| arg == "--scene") {
        xrack_platform_winit::run(particle_mesh_emitters_ui::ParticleSceneShapeDemo::new())
    } else if std::env::args_os().skip(1).any(|arg| arg == "--bounce") {
        xrack_platform_winit::run(particle_mesh_emitters_ui::ParticleSurfaceBounceDemo::new())
    } else {
        xrack_platform_winit::run(particle_mesh_emitters_ui::ParticleMeshEmittersDemo::new())
    }
}
