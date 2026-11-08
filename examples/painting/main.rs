use painting_ui::Gallery;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(Gallery::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
