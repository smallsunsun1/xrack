fn main() -> Result<(), String> {
    xrack_platform_winit::run(reverse_depth_ui::ReverseDepthDemo::new())
}
