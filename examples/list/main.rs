use list_ui::ListDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(ListDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
