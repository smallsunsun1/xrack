fn main() -> Result<(), String> {
    xrack_platform_winit::run(sphere_walk_ui::SphereWalkDemo::new())
}
