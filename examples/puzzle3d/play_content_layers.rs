fn main() -> Result<(), String> {
    if std::env::args_os().skip(1).any(|arg| arg == "--combine") {
        return xrack_platform_winit::run(content_layers_ui::CombinedSceneDemo::new());
    }
    xrack_platform_winit::run(content_layers_ui::ContentLayersDemo::new())
}
