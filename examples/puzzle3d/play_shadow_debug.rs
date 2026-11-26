fn main() -> Result<(), String> {
    xrack_platform_winit::run(shadow_debug_ui::ShadowDebugDemo::new())
}
