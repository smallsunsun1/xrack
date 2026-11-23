fn main() -> Result<(), String> {
    xrack_platform_winit::run(volumetric_fog_ui::VolumetricFogDemo::new())
}
