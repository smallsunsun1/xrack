fn main() -> Result<(), String> {
    xrack_platform_winit::run(light_scope_ui::LightScopeDemo::new())
}
