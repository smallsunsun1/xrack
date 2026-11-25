fn main() -> Result<(), String> {
    xrack_platform_winit::run(body_dynamics_ui::BodyDynamicsDemo::new())
}
