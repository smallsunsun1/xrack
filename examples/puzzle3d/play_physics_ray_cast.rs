fn main() -> Result<(), String> {
    xrack_platform_winit::run(physics_ray_cast_ui::PhysicsRayDemo::new())
}
