fn main() -> Result<(), String> {
    xrack_platform_winit::run(physics_kinematic_ui::PhysicsKinematicDemo::new())
}
