use form_ui::FormDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(FormDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
