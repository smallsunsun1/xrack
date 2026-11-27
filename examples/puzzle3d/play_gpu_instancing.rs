fn main() -> Result<(), String> {
    if std::env::args_os().skip(1).any(|arg| arg == "--catalog") {
        return xrack_platform_winit::run(gpu_instancing_ui::MeshCatalogDemo::new());
    }
    xrack_platform_winit::run(gpu_instancing_ui::GpuInstancingDemo::new())
}
