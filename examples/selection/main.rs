use selection_ui::SelectionDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(SelectionDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
