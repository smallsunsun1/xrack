use dialog_ui::DialogDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(DialogDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
