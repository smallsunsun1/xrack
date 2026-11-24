fn main() -> Result<(), String> {
    xrack_platform_winit::run(gpu_instancing_ui::GpuInstancingDemo::new())
}
