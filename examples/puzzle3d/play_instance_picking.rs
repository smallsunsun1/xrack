fn main() -> Result<(), String> {
    xrack_platform_winit::run(instance_picking_ui::InstancePickingDemo::new())
}
