fn main() -> Result<(), String> {
    match std::env::args_os()
        .nth(1)
        .as_deref()
        .and_then(|arg| arg.to_str())
    {
        Some("--skin") => xrack_platform_winit::run(authored_mesh_ui::AuthoredSkinDemo::new()),
        Some("--morph") => xrack_platform_winit::run(authored_mesh_ui::AuthoredMorphDemo::new()),
        Some("--surface-morph") => {
            xrack_platform_winit::run(authored_mesh_ui::MorphSurfaceDemo::new())
        }
        Some("--scene-particles") => {
            xrack_platform_winit::run(authored_mesh_ui::SceneParticleDemo::new())
        }
        Some("--particle-instances") => {
            xrack_platform_winit::run(authored_mesh_ui::ParticleInstanceDemo::new())
        }
        _ => xrack_platform_winit::run(authored_mesh_ui::AuthoredMeshDemo::new()),
    }
}
