use media_ui::MediaDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(MediaDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
