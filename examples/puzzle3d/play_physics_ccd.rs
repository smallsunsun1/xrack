fn main() -> Result<(), String> {
    xrack_platform_winit::run(physics_ccd_ui::PhysicsCcdDemo::new())
}
