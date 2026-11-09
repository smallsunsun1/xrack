use media_ui::{AnimatedImageDemo, ImageColorizeDemo, MediaDemo, NineSliceDemo, SpriteAtlasDemo};

fn main() -> std::process::ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        Some("--colorize") => xrack_platform_winit::run(ImageColorizeDemo::new()),
        Some("--atlas") => xrack_platform_winit::run(SpriteAtlasDemo::new()),
        Some("--animated") => xrack_platform_winit::run(AnimatedImageDemo::new()),
        Some("--nine-slice") => xrack_platform_winit::run(NineSliceDemo::new()),
        _ => xrack_platform_winit::run(MediaDemo::new()),
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
