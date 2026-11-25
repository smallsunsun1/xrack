fn main() -> Result<(), String> {
    xrack_platform_winit::run(quaternion_scene_ui::QuaternionSceneDemo::new())
}
