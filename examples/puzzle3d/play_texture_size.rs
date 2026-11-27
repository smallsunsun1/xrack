fn main() -> Result<(), String> {
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--render-stats")
    {
        return xrack_platform_winit::run(texture_size_ui::RenderStatsDemo::new());
    }
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--auto-camera")
    {
        return xrack_platform_winit::run(texture_size_ui::AutoCameraDemo::new());
    }
    xrack_platform_winit::run(texture_size_ui::TextureSizeDemo::new())
}
