fn main() -> Result<(), String> {
    xrack_platform_winit::run(random_instances_ui::RandomInstancesDemo::new())
}
