use video_ui::VideoDemo;

fn main() -> Result<(), String> {
    let mut player = VideoDemo::new();
    if let Some(source) = std::env::args().nth(1) {
        let path = if source.starts_with("http://") || source.starts_with("https://") {
            source
        } else {
            std::fs::canonicalize(source)
                .map_err(|error| error.to_string())?
                .into_os_string()
                .into_string()
                .map_err(|_| "media path must be UTF-8")?
        };
        player.set_video_source(path);
    }
    xrack_platform_winit::run(player)
}
