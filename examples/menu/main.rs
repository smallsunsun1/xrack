use menu_ui::MenuDemo;

fn main() -> std::process::ExitCode {
    match xrack_platform_winit::run(MenuDemo::new()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
