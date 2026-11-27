fn main() -> Result<(), String> {
    xrack_platform_winit::run(cube_environment_ui::CubeEnvironmentDemo::new())
}
