fn main() -> Result<(), String> {
    xrack_platform_winit::run(capture_positions_ui::CapturePositionsDemo::new())
}
