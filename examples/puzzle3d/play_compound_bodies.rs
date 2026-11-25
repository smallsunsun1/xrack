fn main() -> Result<(), String> {
    xrack_platform_winit::run(compound_bodies_ui::CompoundBodiesDemo::new())
}
