fn main() -> Result<(), String> {
    xrack_platform_winit::run(custom_pipeline_ui::CustomPipelineDemo::new())
}
