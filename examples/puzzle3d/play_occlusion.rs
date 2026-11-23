fn main() -> Result<(), String> {
    xrack_platform_winit::run(occlusion_ui::OcclusionDemo::new())
}
