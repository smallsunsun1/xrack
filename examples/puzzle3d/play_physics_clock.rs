fn main() -> Result<(), String> {
    xrack_platform_winit::run(physics_clock_ui::PhysicsClockDemo::new())
}
