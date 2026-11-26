fn main() -> Result<(), String> {
    if std::env::args_os()
        .skip(1)
        .any(|arg| arg == "--birth-animation")
    {
        xrack_platform_winit::run(animation_blend_ui::ParticleClipDemo::new())
    } else {
        xrack_platform_winit::run(animation_blend_ui::AnimationBlendDemo::new())
    }
}
