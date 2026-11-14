fn main() -> Result<(), String> {
    xrack_platform_winit::run(live_ui::LocalLiveDemo::new())
}
