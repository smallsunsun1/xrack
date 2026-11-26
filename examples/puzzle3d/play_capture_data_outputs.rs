fn main() -> Result<(), String> {
    xrack_platform_winit::run(capture_data_outputs_ui::CaptureDataOutputsDemo::new())
}
