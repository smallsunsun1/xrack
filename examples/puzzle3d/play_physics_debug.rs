fn main() -> Result<(), String> {
    xrack_platform_winit::run(physics_debug_ui::PhysicsDebugDemo::new())
}
