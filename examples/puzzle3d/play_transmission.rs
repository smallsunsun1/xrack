fn main() -> Result<(), String> {
    xrack_platform_winit::run(transmission_ui::TransmissionDemo::new())
}
