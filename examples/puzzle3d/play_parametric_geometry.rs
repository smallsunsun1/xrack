fn main() -> Result<(), String> {
    xrack_platform_winit::run(parametric_geometry_ui::ParametricGeometryDemo::new())
}
