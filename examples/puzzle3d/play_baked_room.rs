fn main() -> Result<(), String> {
    xrack_platform_winit::run(baked_room_ui::BakedRoomDemo::new())
}
