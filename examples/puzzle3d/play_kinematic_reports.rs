fn main() -> Result<(), String> {
    xrack_platform_winit::run(kinematic_reports_ui::KinematicReportsDemo::new())
}
