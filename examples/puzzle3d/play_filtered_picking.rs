fn main() -> Result<(), String> {
    xrack_platform_winit::run(filtered_picking_ui::FilteredPickingDemo::new())
}
