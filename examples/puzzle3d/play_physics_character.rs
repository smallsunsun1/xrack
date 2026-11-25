fn main() -> Result<(), String> {
    xrack_platform_winit::run(physics_character_ui::PhysicsCharacterDemo::new())
}
