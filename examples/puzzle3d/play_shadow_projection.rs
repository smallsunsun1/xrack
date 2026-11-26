fn main() -> Result<(), String> {
    xrack_platform_winit::run(shadow_projection_ui::ShadowProjectionDemo::new())
}
