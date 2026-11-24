fn main() -> Result<(), String> {
    xrack_platform_winit::run(probe_capture_feed_ui::ProbeCaptureFeedDemo::new())
}
