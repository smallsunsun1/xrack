fn main() -> Result<(), String> {
    xrack_platform_winit::run(probe_controls_ui::ProbeControlsDemo::new())
}
