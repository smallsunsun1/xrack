use split_ui::SplitDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(SplitDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
