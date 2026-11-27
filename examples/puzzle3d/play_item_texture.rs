fn main() -> Result<(), String> {
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--touch-toggles")
    {
        let mut app = item_texture_ui::ControllerTouchDemo::new();
        app.set_toggles_mode(true);
        return xrack_platform_winit::run(app);
    }
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--touch-ranges")
    {
        let mut app = item_texture_ui::ControllerTouchDemo::new();
        app.set_advanced_ranges(true);
        return xrack_platform_winit::run(app);
    }
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--touch-controls")
    {
        let mut app = item_texture_ui::ControllerTouchDemo::new();
        app.set_controls_mode(true);
        return xrack_platform_winit::run(app);
    }
    if std::env::args_os().skip(1).any(|arg| arg == "--touch") {
        return xrack_platform_winit::run(item_texture_ui::ControllerTouchDemo::new());
    }
    if std::env::args_os().skip(1).any(|arg| arg == "--ray") {
        return xrack_platform_winit::run(item_texture_ui::RayItemTextureDemo::new());
    }
    let mut app = item_texture_ui::ItemTextureDemo::new();
    if std::env::args_os().skip(1).any(|arg| arg == "--gestures") {
        app.set_gestures_enabled(true);
        app.set_offscreen_only(true);
    }
    xrack_platform_winit::run(app)
}
